use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGDirectDisplayID;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetDataSize,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSArray, NSError, NSObject};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType, SCWindow,
};
use scopeguard::defer;

use crate::{
    XCapError, XCapResult,
    video_recorder::{Frame, VideoRecorderOptions},
};

/// How long to wait for ScreenCaptureKit before giving up.
const START_TIMEOUT: Duration = Duration::from_secs(5);

fn error_message(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

/// Fetch the shareable content list, which is only available through a
/// completion handler. Blocks the calling thread; that is the capture thread.
fn shareable_content() -> XCapResult<Retained<SCShareableContent>> {
    let (tx, rx) = sync_channel::<Result<Retained<SCShareableContent>, String>>(1);
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = if !error.is_null() {
                Err(error_message(unsafe { &*error }))
            } else if content.is_null() {
                Err("ScreenCaptureKit returned no shareable content".into())
            } else {
                Ok(unsafe { Retained::retain(content) }.expect("shareable content is not null"))
            };
            let _ = tx.send(result);
        },
    );
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };

    match rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(content)) => Ok(content),
        Ok(Err(message)) => Err(XCapError::new(message)),
        Err(_) => Err(XCapError::new(
            "ScreenCaptureKit did not return shareable content in time",
        )),
    }
}

#[derive(Debug)]
struct StreamOutputVars {
    tx: SyncSender<Frame>,
    running: Arc<AtomicBool>,
    created: Instant,
}

impl StreamOutputVars {
    unsafe fn capture(&self, sample_buffer: &CMSampleBuffer) {
        if !self.running.load(Ordering::Acquire) {
            return;
        }
        let timestamp = self.created.elapsed();

        unsafe {
            let Some(pixel_buffer) = CMSampleBuffer::image_buffer(sample_buffer) else {
                return;
            };

            CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
            defer! {
                CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
            };

            let format_type = CVPixelBufferGetPixelFormatType(&pixel_buffer);
            if format_type != kCVPixelFormatType_32BGRA {
                log::error!(
                    "unexpected ScreenCaptureKit pixel format 0x{format_type:08X}, expected BGRA"
                );
                return;
            }

            let width = CVPixelBufferGetWidth(&pixel_buffer);
            let height = CVPixelBufferGetHeight(&pixel_buffer);
            let bytes_per_row = CVPixelBufferGetBytesPerRow(&pixel_buffer);
            let size = CVPixelBufferGetDataSize(&pixel_buffer);
            let base_address = CVPixelBufferGetBaseAddress(&pixel_buffer);
            if base_address.is_null() || width == 0 || height == 0 {
                return;
            }
            let data = slice::from_raw_parts(base_address.cast::<u8>(), size);

            let row_len = width * 4;
            let mut buffer = vec![0; row_len * height];
            for (src_row, dst_row) in data
                .chunks_exact(bytes_per_row)
                .zip(buffer.chunks_exact_mut(row_len))
            {
                for (src, dst) in src_row[..row_len]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst_row.as_chunks_mut::<4>().0)
                {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = src[3];
                }
            }

            // The stream may be stopped while this callback is converting.
            if !self.running.load(Ordering::Acquire) {
                return;
            }

            let _ = self.tx.send(Frame::new_at(
                width as u32,
                height as u32,
                buffer,
                timestamp,
            ));
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "XCapStreamOutput"]
    #[ivars = StreamOutputVars]
    #[derive(Debug)]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        #[allow(non_snake_case)]
        unsafe fn stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            of_type: SCStreamOutputType,
        ) {
            if of_type == SCStreamOutputType::Screen {
                unsafe { self.ivars().capture(sample_buffer) };
            }
        }
    }
);

impl StreamOutput {
    fn new(tx: SyncSender<Frame>, running: Arc<AtomicBool>, created: Instant) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputVars {
            tx,
            running,
            created,
        });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {
    stream: Retained<SCStream>,
    _output: Retained<StreamOutput>,
    _queue: Retained<DispatchQueue>,
    running: Arc<AtomicBool>,
}

impl ImplVideoRecorder {
    pub fn new(
        cg_direct_display_id: CGDirectDisplayID,
        display_width: u32,
        display_height: u32,
        scale_factor: f32,
        options: VideoRecorderOptions,
    ) -> XCapResult<(Self, Receiver<Frame>)> {
        let created = Instant::now();
        let content = shareable_content()?;
        // Excluding the calling process's windows keeps the recorder itself out
        // of the frames; ScreenCaptureKit filters by window objects.
        let excluded: Vec<Retained<SCWindow>> = if options.excludes_own_windows {
            let own_pid = std::process::id() as i32;
            unsafe {
                content
                    .windows()
                    .iter()
                    .filter(|window| window.isOnScreen())
                    .filter(|window| {
                        window
                            .owningApplication()
                            .is_some_and(|app| app.processID() == own_pid)
                    })
                    .collect()
            }
        } else {
            Vec::new()
        };
        let excluded = NSArray::from_retained_slice(&excluded);
        let display = unsafe {
            content
                .displays()
                .iter()
                .find(|display| display.displayID() == cg_direct_display_id)
                .ok_or_else(|| XCapError::new("no shareable display with the requested id"))?
        };

        let scale = if scale_factor > 0. { scale_factor } else { 1. };
        let (source_rect, pixel_width, pixel_height) = match options.region {
            Some(region) => (
                CGRect::new(
                    CGPoint::new(region.x as f64, region.y as f64),
                    CGSize::new(region.width as f64, region.height as f64),
                ),
                (region.width as f32 * scale).round() as usize,
                (region.height as f32 * scale).round() as usize,
            ),
            None => (
                CGRect::new(
                    CGPoint::new(0., 0.),
                    CGSize::new(display_width as f64, display_height as f64),
                ),
                (display_width as f32 * scale).round() as usize,
                (display_height as f32 * scale).round() as usize,
            ),
        };
        if pixel_width == 0 || pixel_height == 0 {
            return Err(XCapError::new("the recording region is empty"));
        }

        let (tx, rx) = sync_channel(0);
        let running = Arc::new(AtomicBool::new(false));
        let output = StreamOutput::new(tx, running.clone(), created);
        let queue = DispatchQueue::new("XCapStreamOutput", DispatchQueueAttr::SERIAL);

        unsafe {
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &excluded,
            );

            let configuration = SCStreamConfiguration::new();
            configuration.setWidth(pixel_width);
            configuration.setHeight(pixel_height);
            configuration.setPixelFormat(kCVPixelFormatType_32BGRA);
            configuration.setSourceRect(source_rect);
            configuration.setScalesToFit(false);
            configuration.setShowsCursor(options.captures_cursor);
            configuration.setQueueDepth(5);
            configuration.setMinimumFrameInterval(CMTime::new(1, options.fps.clamp(1, 60) as i32));

            let stream = SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &configuration,
                None,
            );

            let protocol: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    protocol,
                    SCStreamOutputType::Screen,
                    Some(&queue),
                )
                .map_err(|error| XCapError::new(error_message(&error)))?;

            Ok((
                ImplVideoRecorder {
                    stream,
                    _output: output,
                    _queue: queue.into(),
                    running,
                },
                rx,
            ))
        }
    }

    pub fn start(&self) -> XCapResult<()> {
        self.running.store(true, Ordering::Release);
        let (tx, rx) = sync_channel::<Result<(), String>>(1);
        let handler = RcBlock::new(move |error: *mut NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                Err(error_message(unsafe { &*error }))
            };
            let _ = tx.send(result);
        });
        unsafe {
            self.stream
                .startCaptureWithCompletionHandler(Some(&handler))
        };

        match rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => {
                self.running.store(false, Ordering::Release);
                Err(XCapError::new(message))
            }
            Err(_) => {
                self.running.store(false, Ordering::Release);
                Err(XCapError::new("ScreenCaptureKit did not start in time"))
            }
        }
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.running.store(false, Ordering::Release);
        let (tx, rx) = sync_channel(1);
        let handler = RcBlock::new(move |_: *mut NSError| {
            let _ = tx.send(());
        });
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&handler)) };
        let _ = rx.recv_timeout(START_TIMEOUT);

        Ok(())
    }
}
