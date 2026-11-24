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
use objc2_core_graphics::{CGColorSpace, CGDirectDisplayID};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetDataSize,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVImageBufferCGColorSpaceKey, kCVPixelFormatType_32BGRA, kCVPixelFormatType_64RGBAHalf,
};
use objc2_foundation::{NSArray, NSError, NSObject};
use objc2_screen_capture_kit::{
    SCCaptureDynamicRange, SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};
use scopeguard::defer;

use super::hdr;
use crate::{
    HdrImage, XCapError, XCapResult,
    video_recorder::{DynamicRange, Frame, OutputSize, VideoRecorderOptions},
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
    // Restrict enumeration to on-screen, non-desktop windows: on a busy
    // desktop the full list carries ~150 off-screen windows we immediately
    // discard, and the extra marshalling costs ~15 ms on every call (much
    // more under load). `displays()` is unaffected, and the caller's own
    // windows are on-screen, so the exclusion list stays identical.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        )
    };

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
    /// Whether the stream was asked for extended range, i.e. whether an RGhA
    /// buffer is expected at all.
    hdr: bool,
    /// Extended sRGB → linear lookup, built once per recording.
    srgb_lut: Option<Box<[u16; 65_536]>>,
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
            let width = CVPixelBufferGetWidth(&pixel_buffer);
            let height = CVPixelBufferGetHeight(&pixel_buffer);
            let bytes_per_row = CVPixelBufferGetBytesPerRow(&pixel_buffer);
            let size = CVPixelBufferGetDataSize(&pixel_buffer);
            let base_address = CVPixelBufferGetBaseAddress(&pixel_buffer);
            if base_address.is_null() || width == 0 || height == 0 {
                return;
            }
            let data = slice::from_raw_parts(base_address.cast::<u8>(), size);

            let frame = match format_type {
                kCVPixelFormatType_32BGRA => {
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
                    Frame::new_at(width as u32, height as u32, buffer, timestamp)
                }
                kCVPixelFormatType_64RGBAHalf if self.hdr => {
                    let linear = pixel_buffer
                        .attachment(kCVImageBufferCGColorSpaceKey, std::ptr::null_mut())
                        .and_then(|space| space.downcast::<CGColorSpace>().ok())
                        .is_some_and(|space| hdr::is_linear(Some(&space)));
                    let row_len = width * 8;
                    let mut pixels = Vec::with_capacity(width * height * 6);
                    for src_row in data.chunks_exact(bytes_per_row).take(height) {
                        for pixel in src_row[..row_len].chunks_exact(8).take(width) {
                            for bytes in pixel.as_chunks::<2>().0.iter().take(3) {
                                let bits = u16::from_le_bytes(*bytes);
                                let out = if linear {
                                    bits
                                } else {
                                    self.srgb_lut
                                        .as_ref()
                                        .map_or(bits, |lut| lut[bits as usize])
                                };
                                pixels.extend_from_slice(&out.to_le_bytes());
                            }
                        }
                    }
                    Frame::new_hdr(
                        width as u32,
                        height as u32,
                        HdrImage::new(width as u32, height as u32, pixels, 203.0),
                        timestamp,
                    )
                }
                _ => {
                    log::error!(
                        "unexpected ScreenCaptureKit pixel format 0x{format_type:08X}, expected BGRA or RGhA"
                    );
                    return;
                }
            };

            // The stream may be stopped while this callback is converting.
            if !self.running.load(Ordering::Acquire) {
                return;
            }

            // Never block the SCK callback: stopCapture waits for it to return.
            let _ = self.tx.try_send(frame);
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
    fn new(
        tx: SyncSender<Frame>,
        running: Arc<AtomicBool>,
        created: Instant,
        hdr: bool,
        srgb_lut: Option<Box<[u16; 65_536]>>,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputVars {
            tx,
            running,
            created,
            hdr,
            srgb_lut,
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
        // Keep both spaces side by side: `sourceRect` and the logical size are
        // in display points, while the native frame is that rectangle scaled
        // into pixels. Callers only ever supply the former.
        let (source_rect, logical_width, logical_height, pixel_width, pixel_height) =
            match options.region {
                Some(region) => (
                    CGRect::new(
                        CGPoint::new(region.x as f64, region.y as f64),
                        CGSize::new(region.width as f64, region.height as f64),
                    ),
                    region.width as usize,
                    region.height as usize,
                    (region.width as f32 * scale).round() as usize,
                    (region.height as f32 * scale).round() as usize,
                ),
                None => (
                    CGRect::new(
                        CGPoint::new(0., 0.),
                        CGSize::new(display_width as f64, display_height as f64),
                    ),
                    display_width as usize,
                    display_height as usize,
                    (display_width as f32 * scale).round() as usize,
                    (display_height as f32 * scale).round() as usize,
                ),
            };
        if pixel_width == 0 || pixel_height == 0 {
            return Err(XCapError::new("the recording region is empty"));
        }
        // `width`/`height` are the *output* dimensions in pixels while
        // `sourceRect` stays in display points. Asking for `Logical` means the
        // output is exactly the source rectangle's point size, so the height
        // needs no derivation: ScreenCaptureKit downscales in the compositor
        // before the sample buffer reaches our callback, and the rest of the
        // pipeline sees a quarter of the pixels on a 2x Retina display.
        // `scalesToFit = false` keeps the frame from being letterboxed. Only
        // shrink: at a scale factor of 1 there is nothing to downscale.
        let (output_width, output_height) = match options.output_size {
            OutputSize::Logical
                if logical_width > 0
                    && logical_height > 0
                    && logical_width < pixel_width
                    && logical_height < pixel_height =>
            {
                (logical_width, logical_height)
            }
            _ => (pixel_width, pixel_height),
        };

        let (tx, rx) = sync_channel(4);
        let running = Arc::new(AtomicBool::new(false));
        // HDR streaming needs Apple Silicon and macOS 15; asking on an older
        // Mac silently keeps the SDR stream, so only build the lookup when the
        // frames can actually arrive as RGhA.
        let hdr_enabled = options.dynamic_range == DynamicRange::Hdr && hdr::supported();
        let srgb_lut = hdr_enabled.then(hdr::srgb_to_linear_lut);
        let output = StreamOutput::new(tx, running.clone(), created, hdr_enabled, srgb_lut);
        let queue = DispatchQueue::new("XCapStreamOutput", DispatchQueueAttr::SERIAL);

        unsafe {
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &excluded,
            );

            let configuration = SCStreamConfiguration::new();
            configuration.setWidth(output_width);
            configuration.setHeight(output_height);
            if hdr_enabled {
                // The configuration picks the RGhA pixel format and the
                // extended-range color space for us.
                configuration.setCaptureDynamicRange(SCCaptureDynamicRange::HDRLocalDisplay);
            } else {
                configuration.setPixelFormat(kCVPixelFormatType_32BGRA);
            }
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
