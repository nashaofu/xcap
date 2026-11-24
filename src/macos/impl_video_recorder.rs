use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
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
use objc2_core_foundation::{CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColorSpace, CGDirectDisplayID};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetDataSize,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVImageBufferCGColorSpaceKey, kCVImageBufferYCbCrMatrix_ITU_R_601_4,
    kCVImageBufferYCbCrMatrixKey, kCVPixelFormatType_32BGRA as PIXEL_FORMAT_32BGRA,
    kCVPixelFormatType_64RGBAHalf as PIXEL_FORMAT_64RGBA_HALF,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as PIXEL_FORMAT_420V,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum YcbcrMatrix {
    Bt601,
    Bt709,
}

fn ycbcr_matrix(pixel_buffer: &CVPixelBuffer) -> YcbcrMatrix {
    let matrix = unsafe {
        pixel_buffer
            .attachment(kCVImageBufferYCbCrMatrixKey, std::ptr::null_mut())
            .and_then(|value| value.downcast::<CFString>().ok())
    };
    let bt601 = unsafe { kCVImageBufferYCbCrMatrix_ITU_R_601_4 };
    if matrix
        .as_deref()
        .is_some_and(|value| value.to_string() == bt601.to_string())
    {
        YcbcrMatrix::Bt601
    } else {
        YcbcrMatrix::Bt709
    }
}

fn rgba_from_420v(
    width: usize,
    height: usize,
    luma: &[u8],
    luma_stride: usize,
    chroma: &[u8],
    chroma_stride: usize,
    matrix: YcbcrMatrix,
) -> Vec<u8> {
    let (cb_green, cb_blue, cr_red, cr_green) = match matrix {
        YcbcrMatrix::Bt601 => (-0.391_762_3, 2.017_232_1, 1.596_026_8, -0.812_967_6),
        YcbcrMatrix::Bt709 => (-0.187_324_3, 2.112_401_8, 1.792_741_1, -0.532_909_3),
    };
    let mut luma_lut = [0i32; 256];
    let mut cb_green_lut = [0i32; 256];
    let mut cb_blue_lut = [0i32; 256];
    let mut cr_red_lut = [0i32; 256];
    let mut cr_green_lut = [0i32; 256];
    for value in 0..256 {
        luma_lut[value] = ((value as f32 - 16.0) * 1.164_383_6).round() as i32;
        let chroma_value = value as f32 - 128.0;
        cb_green_lut[value] = (chroma_value * cb_green).round() as i32;
        cb_blue_lut[value] = (chroma_value * cb_blue).round() as i32;
        cr_red_lut[value] = (chroma_value * cr_red).round() as i32;
        cr_green_lut[value] = (chroma_value * cr_green).round() as i32;
    }

    let chroma_width = width.div_ceil(2);
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        let luma_row = &luma[y * luma_stride..y * luma_stride + width];
        let chroma_row =
            &chroma[(y / 2) * chroma_stride..(y / 2) * chroma_stride + chroma_width * 2];
        for x in 0..width {
            let cb = chroma_row[(x / 2) * 2] as usize;
            let cr = chroma_row[(x / 2) * 2 + 1] as usize;
            let luma = luma_lut[luma_row[x] as usize];
            let out = &mut rgba[(y * width + x) * 4..(y * width + x + 1) * 4];
            out[0] = (luma + cr_red_lut[cr]).clamp(0, 255) as u8;
            out[1] = (luma + cb_green_lut[cb] + cr_green_lut[cr]).clamp(0, 255) as u8;
            out[2] = (luma + cb_blue_lut[cb]).clamp(0, 255) as u8;
            out[3] = 255;
        }
    }
    rgba
}

#[derive(Debug)]
struct StreamOutputVars {
    tx: SyncSender<Frame>,
    running: Arc<AtomicBool>,
    created: Instant,
    /// Whether RGhA frames should be decoded as extended-range sRGB.
    hdr: bool,
    /// Extended sRGB → linear lookup, built once per recording.
    srgb_lut: Option<Box<[u16; 65_536]>>,
    biplanar_logged: AtomicBool,
    unsupported_format_logged: AtomicU32,
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

            if CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags::ReadOnly) != 0 {
                return;
            }
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
                PIXEL_FORMAT_32BGRA => {
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
                PIXEL_FORMAT_420V => {
                    let matrix = ycbcr_matrix(&pixel_buffer);
                    if self
                        .biplanar_logged
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                    {
                        log::info!(
                            "ScreenCaptureKit delivered SDR 420v; recording as SDR (hdr_requested={} matrix={matrix:?})",
                            self.hdr
                        );
                    }
                    let chroma_width = width.div_ceil(2);
                    let chroma_height = height.div_ceil(2);
                    let luma_stride = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 0);
                    let chroma_stride = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 1);
                    let chroma_address = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 1);
                    if chroma_address.is_null() {
                        return;
                    }
                    let luma_len = luma_stride * height;
                    let chroma_len = (chroma_height - 1) * chroma_stride + chroma_width * 2;
                    let luma = slice::from_raw_parts(base_address.cast::<u8>(), luma_len);
                    let chroma = slice::from_raw_parts(chroma_address.cast::<u8>(), chroma_len);
                    Frame::new_at(
                        width as u32,
                        height as u32,
                        rgba_from_420v(
                            width,
                            height,
                            luma,
                            luma_stride,
                            chroma,
                            chroma_stride,
                            matrix,
                        ),
                        timestamp,
                    )
                }
                PIXEL_FORMAT_64RGBA_HALF if self.hdr => {
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
                    if self
                        .unsupported_format_logged
                        .compare_exchange(0, format_type, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                    {
                        log::error!(
                            "unexpected ScreenCaptureKit pixel format 0x{format_type:08X}, expected BGRA, 420v or RGhA"
                        );
                    }
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
            biplanar_logged: AtomicBool::new(false),
            unsupported_format_logged: AtomicU32::new(0),
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
                configuration.setPixelFormat(PIXEL_FORMAT_32BGRA);
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
        let handler = RcBlock::new(move |error: *mut NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                Err(error_message(unsafe { &*error }))
            };
            let _ = tx.send(result);
        });
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&handler)) };
        match rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(XCapError::new(message)),
            Err(RecvTimeoutError::Timeout) => {
                Err(XCapError::new("ScreenCaptureKit did not stop in time"))
            }
            Err(RecvTimeoutError::Disconnected) => Err(XCapError::new(
                "ScreenCaptureKit stop completion was dropped",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biplanar_video_range_black_white_and_neutral_decode_to_rgba() {
        let mut luma = vec![0; 8 * 2];
        luma[..4].copy_from_slice(&[16, 16, 16, 16]);
        luma[8..12].copy_from_slice(&[235, 235, 235, 235]);
        let rgba = rgba_from_420v(4, 2, &luma, 8, &[128, 128, 128, 128], 6, YcbcrMatrix::Bt709);
        assert_eq!(&rgba[..16], &[0, 0, 0, 255].repeat(4));
        assert_eq!(&rgba[16..], &[255, 255, 255, 255].repeat(4));

        let neutral = rgba_from_420v(
            2,
            2,
            &[126, 126, 126, 126],
            2,
            &[128, 128],
            2,
            YcbcrMatrix::Bt709,
        );
        assert_eq!(neutral, [128, 128, 128, 255].repeat(4));
    }

    #[test]
    fn each_chroma_pair_colours_its_two_by_two_block() {
        let luma = [63; 16];
        let rgba = rgba_from_420v(
            4,
            4,
            &luma,
            4,
            &[90, 240, 90, 240, 90, 240, 90, 240],
            4,
            YcbcrMatrix::Bt709,
        );
        for y in 0..4 {
            for x in 0..2 {
                let at = (y * 4 + x) * 4;
                let pixel = &rgba[at..at + 3];
                assert!(pixel[0] > 250 && pixel[1] < 5 && pixel[2] < 5);
            }
        }
    }

    #[test]
    fn bt601_matrix_decodes_its_reference_colours() {
        let rgba = rgba_from_420v(
            6,
            1,
            &[81, 81, 145, 145, 41, 41],
            6,
            &[90, 240, 54, 30, 240, 110],
            6,
            YcbcrMatrix::Bt601,
        );
        let expected = [
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 255, 1, 255],
            [0, 255, 1, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
        ];
        for (pixel, expected) in rgba.as_chunks::<4>().0.iter().zip(expected) {
            for (value, expected) in pixel.iter().zip(expected) {
                assert!(
                    (i32::from(*value) - expected).abs() <= 2,
                    "{pixel:?} component {value} != {expected}"
                );
            }
        }
    }

    #[test]
    fn odd_dimensions_use_rounded_up_chroma_planes() {
        let rgba = rgba_from_420v(
            3,
            3,
            &[126; 9],
            3,
            &[128, 128, 128, 128, 128, 128, 128, 128],
            4,
            YcbcrMatrix::Bt709,
        );
        assert_eq!(rgba.len(), 3 * 3 * 4);
        assert!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p == &[128, 128, 128, 255])
        );
    }
}
