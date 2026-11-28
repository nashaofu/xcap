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
    video_recorder::{Frame, FrameBufferPool, OutputSize, VideoRecorderOptions},
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

fn bgra_to_rgba_into(
    data: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
    pool: Option<&FrameBufferPool>,
) -> Vec<u8> {
    let row_len = width * 4;
    let total_len = row_len * height;
    let mut buffer = pool.map_or_else(|| Vec::with_capacity(total_len), |p| p.take(total_len));
    debug_assert!(buffer.capacity() >= total_len);
    let dst = &mut buffer.spare_capacity_mut()[..total_len];
    for y in 0..height {
        let src_row = &data[y * bytes_per_row..y * bytes_per_row + row_len];
        let dst_row = &mut dst[y * row_len..(y + 1) * row_len];
        let mut offset = 0usize;
        #[cfg(target_arch = "aarch64")]
        {
            use std::arch::aarch64::{vld4q_u8, vst4q_u8};
            while offset + 64 <= row_len {
                // SAFETY: `offset + 64 <= row_len`, both `src_row` and `dst_row`
                // cover at least `row_len` bytes, and `vld4q_u8`/`vst4q_u8` accept
                // unaligned byte pointers.
                unsafe {
                    let mut px = vld4q_u8(src_row.as_ptr().add(offset));
                    std::mem::swap(&mut px.0, &mut px.2);
                    vst4q_u8(dst_row.as_mut_ptr().add(offset).cast::<u8>(), px);
                }
                offset += 64;
            }
        }
        while offset + 4 <= row_len {
            dst_row[offset].write(src_row[offset + 2]);
            dst_row[offset + 1].write(src_row[offset + 1]);
            dst_row[offset + 2].write(src_row[offset]);
            dst_row[offset + 3].write(src_row[offset + 3]);
            offset += 4;
        }
    }
    // SAFETY: Every byte in `0..total_len` was initialized row-by-row above.
    unsafe { buffer.set_len(total_len) };
    buffer
}

#[derive(Debug)]
struct StreamOutputVars {
    tx: SyncSender<Frame>,
    running: Arc<AtomicBool>,
    created: Instant,
    buffer_pool: Option<FrameBufferPool>,
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
            let Some(row_len) = width.checked_mul(4) else {
                return;
            };
            let Some(min_size) = height
                .checked_sub(1)
                .and_then(|last| last.checked_mul(bytes_per_row))
                .and_then(|offset| offset.checked_add(row_len))
            else {
                return;
            };
            if bytes_per_row < row_len || size < min_size {
                return;
            }
            let data = slice::from_raw_parts(base_address.cast::<u8>(), size);
            let buffer = bgra_to_rgba_into(
                data,
                width,
                height,
                bytes_per_row,
                self.buffer_pool.as_ref(),
            );

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
    fn new(
        tx: SyncSender<Frame>,
        running: Arc<AtomicBool>,
        created: Instant,
        buffer_pool: Option<FrameBufferPool>,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputVars {
            tx,
            running,
            created,
            buffer_pool,
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

        let channel_cap = options.channel_capacity.map(|c| c.clamp(1, 16));
        let (tx, rx) = sync_channel(channel_cap.unwrap_or(0));
        let running = Arc::new(AtomicBool::new(false));
        let output = StreamOutput::new(tx, running.clone(), created, options.buffer_pool.clone());
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
            configuration.setPixelFormat(kCVPixelFormatType_32BGRA);
            configuration.setSourceRect(source_rect);
            configuration.setScalesToFit(false);
            configuration.setShowsCursor(options.captures_cursor);
            configuration.setQueueDepth(if channel_cap.is_some_and(|c| c <= 2) {
                3
            } else {
                5
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_handles_padding_tails_and_pool_reuse() {
        let pool = FrameBufferPool::new(2);
        for width in [1usize, 15, 16, 17, 33] {
            let height = 3;
            let row_len = width * 4;
            let stride = row_len + 12;
            let mut src = vec![0xAAu8; stride * height];
            let mut expected = Vec::with_capacity(row_len * height);
            for y in 0..height {
                for x in 0..width {
                    let b = ((x * 7 + y * 13) & 0xff) as u8;
                    let g = ((x * 11 + y * 5 + 40) & 0xff) as u8;
                    let r = ((x * 19 + y * 3 + 90) & 0xff) as u8;
                    let a = ((x + y + 200) & 0xff) as u8;
                    let off = y * stride + x * 4;
                    src[off..off + 4].copy_from_slice(&[b, g, r, a]);
                    expected.extend_from_slice(&[r, g, b, a]);
                }
            }

            let out = bgra_to_rgba_into(&src, width, height, stride, Some(&pool));
            assert_eq!(out, expected, "width={width}");
            let ptr = out.as_ptr();
            pool.recycle(out);

            let reused = bgra_to_rgba_into(&src, width, height, stride, Some(&pool));
            assert_eq!(reused, expected, "width={width}");
            assert_eq!(
                reused.as_ptr(),
                ptr,
                "expected buffer reuse for width={width}"
            );
        }
    }
}
