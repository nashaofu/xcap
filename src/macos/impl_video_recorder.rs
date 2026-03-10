use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::{
    AllocAnyThread, DefinedClass, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureOutput, AVCaptureScreenInput, AVCaptureSession,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
};
use objc2_core_graphics::CGDirectDisplayID;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetDataSize, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferGetWidthOfPlane, CVPixelBufferIsPlanar, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32ARGB, kCVPixelFormatType_32BGRA, kCVPixelFormatType_32RGBA,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_422YpCbCr8,
    kCVPixelFormatType_422YpCbCr8_yuvs,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use scopeguard::defer;

use crate::{XCapError, XCapResult, video_recorder::Frame};

#[derive(Debug, Clone)]
struct DataOutputSampleBufferDelegateVars {
    tx: SyncSender<Frame>,
    running: Arc<AtomicBool>,
}

impl DataOutputSampleBufferDelegateVars {
    fn pixel_format_name(format_type: u32) -> &'static str {
        if format_type == kCVPixelFormatType_32ARGB {
            "kCVPixelFormatType_32ARGB"
        } else if format_type == kCVPixelFormatType_32BGRA {
            "kCVPixelFormatType_32BGRA"
        } else if format_type == kCVPixelFormatType_32RGBA {
            "kCVPixelFormatType_32RGBA"
        } else if format_type == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
            "kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange"
        } else if format_type == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
            "kCVPixelFormatType_420YpCbCr8BiPlanarFullRange"
        } else if format_type == kCVPixelFormatType_422YpCbCr8 {
            "kCVPixelFormatType_422YpCbCr8"
        } else if format_type == kCVPixelFormatType_422YpCbCr8_yuvs {
            "kCVPixelFormatType_422YpCbCr8_yuvs"
        } else {
            "unknown"
        }
    }

    fn is_supported_format(format_type: u32) -> bool {
        [
            kCVPixelFormatType_32ARGB,
            kCVPixelFormatType_32BGRA,
            kCVPixelFormatType_32RGBA,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            kCVPixelFormatType_422YpCbCr8,
            kCVPixelFormatType_422YpCbCr8_yuvs,
        ]
        .contains(&format_type)
    }

    fn clamp_to_u8(value: i32) -> u8 {
        value.clamp(0, 255) as u8
    }

    fn yuv_to_rgb_video_range(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
        let c = i32::from(y).saturating_sub(16);
        let d = i32::from(u) - 128;
        let e = i32::from(v) - 128;

        let r = (298 * c + 409 * e + 128) >> 8;
        let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
        let b = (298 * c + 516 * d + 128) >> 8;

        (
            Self::clamp_to_u8(r),
            Self::clamp_to_u8(g),
            Self::clamp_to_u8(b),
        )
    }

    fn yuv_to_rgb_full_range(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
        let c = i32::from(y);
        let d = i32::from(u) - 128;
        let e = i32::from(v) - 128;

        let r = c + ((359 * e + 128) >> 8);
        let g = c - ((88 * d + 183 * e + 128) >> 8);
        let b = c + ((454 * d + 128) >> 8);

        (
            Self::clamp_to_u8(r),
            Self::clamp_to_u8(g),
            Self::clamp_to_u8(b),
        )
    }

    fn copy_packed_rows(width: usize, height: usize, bytes_per_row: usize, data: &[u8]) -> Vec<u8> {
        let row_len = width * 4;
        let mut buffer = vec![0; row_len * height];

        for row_index in 0..height {
            let src_row_start = row_index * bytes_per_row;
            let dst_row_start = row_index * row_len;
            let src_row = &data[src_row_start..src_row_start + row_len];
            let dst_row = &mut buffer[dst_row_start..dst_row_start + row_len];
            dst_row.copy_from_slice(src_row);
        }

        buffer
    }

    fn yuv422_to_rgba(width: usize, height: usize, bytes_per_row: usize, data: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0; width * height * 4];
        let src_row_len = width * 2;
        let dst_row_len = width * 4;

        for row_index in 0..height {
            let src_row_start = row_index * bytes_per_row;
            let dst_row_start = row_index * dst_row_len;
            let src_row = &data[src_row_start..src_row_start + src_row_len];
            let dst_row = &mut buffer[dst_row_start..dst_row_start + dst_row_len];

            for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(8)) {
                let u = src[0];
                let y0 = src[1];
                let v = src[2];
                let y1 = src[3];

                let (r0, g0, b0) = Self::yuv_to_rgb_video_range(y0, u, v);
                dst[0] = r0;
                dst[1] = g0;
                dst[2] = b0;
                dst[3] = 255;

                let (r1, g1, b1) = Self::yuv_to_rgb_video_range(y1, u, v);
                dst[4] = r1;
                dst[5] = g1;
                dst[6] = b1;
                dst[7] = 255;
            }
        }

        buffer
    }

    fn yuyv422_to_rgba(width: usize, height: usize, bytes_per_row: usize, data: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0; width * height * 4];
        let src_row_len = width * 2;
        let dst_row_len = width * 4;

        for row_index in 0..height {
            let src_row_start = row_index * bytes_per_row;
            let dst_row_start = row_index * dst_row_len;
            let src_row = &data[src_row_start..src_row_start + src_row_len];
            let dst_row = &mut buffer[dst_row_start..dst_row_start + dst_row_len];

            for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(8)) {
                let y0 = src[0];
                let u = src[1];
                let y1 = src[2];
                let v = src[3];

                let (r0, g0, b0) = Self::yuv_to_rgb_video_range(y0, u, v);
                dst[0] = r0;
                dst[1] = g0;
                dst[2] = b0;
                dst[3] = 255;

                let (r1, g1, b1) = Self::yuv_to_rgb_video_range(y1, u, v);
                dst[4] = r1;
                dst[5] = g1;
                dst[6] = b1;
                dst[7] = 255;
            }
        }

        buffer
    }

    fn bgra_to_rgba(width: usize, height: usize, bytes_per_row: usize, data: &[u8]) -> Vec<u8> {
        let row_len = width * 4;
        let mut buffer = vec![0; row_len * height];

        for row_index in 0..height {
            let src_row_start = row_index * bytes_per_row;
            let dst_row_start = row_index * row_len;
            let src_row = &data[src_row_start..src_row_start + row_len];
            let dst_row = &mut buffer[dst_row_start..dst_row_start + row_len];

            let mut offset = 0;
            while offset < row_len {
                dst_row[offset] = src_row[offset + 2];
                dst_row[offset + 1] = src_row[offset + 1];
                dst_row[offset + 2] = src_row[offset];
                dst_row[offset + 3] = src_row[offset + 3];
                offset += 4;
            }
        }

        buffer
    }

    fn argb_to_rgba(width: usize, height: usize, bytes_per_row: usize, data: &[u8]) -> Vec<u8> {
        let row_len = width * 4;
        let mut buffer = vec![0; row_len * height];

        for row_index in 0..height {
            let src_row_start = row_index * bytes_per_row;
            let dst_row_start = row_index * row_len;
            let src_row = &data[src_row_start..src_row_start + row_len];
            let dst_row = &mut buffer[dst_row_start..dst_row_start + row_len];

            let mut offset = 0;
            while offset < row_len {
                dst_row[offset] = src_row[offset + 1];
                dst_row[offset + 1] = src_row[offset + 2];
                dst_row[offset + 2] = src_row[offset + 3];
                dst_row[offset + 3] = src_row[offset];
                offset += 4;
            }
        }

        buffer
    }

    fn nv12_to_rgba(
        width: usize,
        height: usize,
        y_bytes_per_row: usize,
        uv_bytes_per_row: usize,
        y_plane: &[u8],
        uv_plane: &[u8],
        full_range: bool,
    ) -> Vec<u8> {
        let mut buffer = vec![0; width * height * 4];

        for row_index in 0..height {
            let y_row_start = row_index * y_bytes_per_row;
            let uv_row_start = (row_index / 2) * uv_bytes_per_row;
            let dst_row_start = row_index * width * 4;

            for column_index in 0..width {
                let y = y_plane[y_row_start + column_index];
                let uv_offset = uv_row_start + (column_index / 2) * 2;
                let u = uv_plane[uv_offset];
                let v = uv_plane[uv_offset + 1];

                let (r, g, b) = if full_range {
                    Self::yuv_to_rgb_full_range(y, u, v)
                } else {
                    Self::yuv_to_rgb_video_range(y, u, v)
                };

                let dst_offset = dst_row_start + column_index * 4;
                buffer[dst_offset] = r;
                buffer[dst_offset + 1] = g;
                buffer[dst_offset + 2] = b;
                buffer[dst_offset + 3] = 255;
            }
        }

        buffer
    }

    fn capture(
        &self,
        _output: &AVCaptureOutput,
        sample_buffer: &CMSampleBuffer,
        _connection: &AVCaptureConnection,
    ) {
        if !self.running.load(Ordering::Acquire) {
            return;
        }

        unsafe {
            let pixel_buffer = match CMSampleBuffer::image_buffer(sample_buffer) {
                Some(pixel_buffer) => pixel_buffer,
                None => return,
            };

            CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
            defer! {
                CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
            };

            let format_type = CVPixelBufferGetPixelFormatType(&pixel_buffer);

            if !Self::is_supported_format(format_type) {
                log::error!(
                    "pixel format type 0x{format_type:08X} ({}) is not supported",
                    Self::pixel_format_name(format_type)
                );
                return;
            }

            let width = CVPixelBufferGetWidth(&pixel_buffer);
            let height = CVPixelBufferGetHeight(&pixel_buffer);
            let buffer = if format_type == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
                || format_type == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            {
                if !CVPixelBufferIsPlanar(&pixel_buffer) {
                    log::error!(
                        "pixel format type 0x{format_type:08X} ({}) is expected to be planar",
                        Self::pixel_format_name(format_type)
                    );
                    return;
                }

                let y_width = CVPixelBufferGetWidthOfPlane(&pixel_buffer, 0);
                let y_height = CVPixelBufferGetHeightOfPlane(&pixel_buffer, 0);
                let uv_height = CVPixelBufferGetHeightOfPlane(&pixel_buffer, 1);
                let y_bytes_per_row = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 0);
                let uv_bytes_per_row = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 1);
                let y_base_address = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 0);
                let uv_base_address = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 1);

                if y_base_address.is_null() || uv_base_address.is_null() {
                    log::error!(
                        "pixel format type 0x{format_type:08X} ({}) returned a null plane base address",
                        Self::pixel_format_name(format_type)
                    );
                    return;
                }

                let y_plane =
                    slice::from_raw_parts(y_base_address.cast::<u8>(), y_bytes_per_row * y_height);
                let uv_plane = slice::from_raw_parts(
                    uv_base_address.cast::<u8>(),
                    uv_bytes_per_row * uv_height,
                );

                Self::nv12_to_rgba(
                    y_width,
                    y_height,
                    y_bytes_per_row,
                    uv_bytes_per_row,
                    y_plane,
                    uv_plane,
                    format_type == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                )
            } else {
                let bytes_per_row = CVPixelBufferGetBytesPerRow(&pixel_buffer);
                let base_address = CVPixelBufferGetBaseAddress(&pixel_buffer);
                let size = CVPixelBufferGetDataSize(&pixel_buffer);

                if base_address.is_null() {
                    log::error!(
                        "pixel format type 0x{format_type:08X} ({}) returned a null base address",
                        Self::pixel_format_name(format_type)
                    );
                    return;
                }

                let data = slice::from_raw_parts(base_address.cast::<u8>(), size);

                if format_type == kCVPixelFormatType_32RGBA {
                    Self::copy_packed_rows(width, height, bytes_per_row, data)
                } else if format_type == kCVPixelFormatType_32BGRA {
                    Self::bgra_to_rgba(width, height, bytes_per_row, data)
                } else if format_type == kCVPixelFormatType_32ARGB {
                    Self::argb_to_rgba(width, height, bytes_per_row, data)
                } else if format_type == kCVPixelFormatType_422YpCbCr8 {
                    Self::yuv422_to_rgba(width, height, bytes_per_row, data)
                } else if format_type == kCVPixelFormatType_422YpCbCr8_yuvs {
                    Self::yuyv422_to_rgba(width, height, bytes_per_row, data)
                } else {
                    unreachable!()
                }
            };

            // stop 之后，队列里可能还有回调在做像素转换；发送前再检查一次。
            if !self.running.load(Ordering::Acquire) {
                return;
            }

            let _ = self.tx.send(Frame {
                width: width as u32,
                height: height as u32,
                raw: buffer,
            });
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "DataOutputSampleBufferDelegate"]
    #[ivars = DataOutputSampleBufferDelegateVars]
    #[derive(Debug)]
    struct DataOutputSampleBufferDelegate;

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for DataOutputSampleBufferDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            connection: &AVCaptureConnection,
        ) {
            self.ivars().capture(output, sample_buffer, connection);
        }
    }
);

unsafe impl NSObjectProtocol for DataOutputSampleBufferDelegate {}

impl DataOutputSampleBufferDelegate {
    fn new(tx: SyncSender<Frame>, running: Arc<AtomicBool>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DataOutputSampleBufferDelegateVars { tx, running });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {
    session: Retained<AVCaptureSession>,
    _input: Retained<AVCaptureScreenInput>,
    _output: Retained<AVCaptureVideoDataOutput>,
    _delegate: Retained<DataOutputSampleBufferDelegate>,
    running: Arc<AtomicBool>,
}

impl ImplVideoRecorder {
    pub fn new(cg_direct_display_id: CGDirectDisplayID) -> XCapResult<(Self, Receiver<Frame>)> {
        unsafe {
            let session = AVCaptureSession::new();
            let input = AVCaptureScreenInput::initWithDisplayID(
                AVCaptureScreenInput::alloc(),
                cg_direct_display_id,
            )
            .ok_or(XCapError::new(
                "AVCaptureScreenInput::initWithDisplayID failed",
            ))?;
            input.setCapturesCursor(true);
            input.setCapturesMouseClicks(true);
            let min_frame_duration = CMTime::new(1, 60);
            let _: () = msg_send![&input, setMinFrameDuration: min_frame_duration];

            if session.canAddInput(&input) {
                session.addInput(&input);
            }

            let output = AVCaptureVideoDataOutput::new();
            output.setAlwaysDiscardsLateVideoFrames(true);
            output.setAutomaticallyConfiguresOutputBufferDimensions(true);

            let format_type_key =
                NSString::from_str(kCVPixelBufferPixelFormatTypeKey.to_string().as_str());
            let available_format_types = output.availableVideoCVPixelFormatTypes();
            let preferred_format_types = [
                kCVPixelFormatType_422YpCbCr8,
                kCVPixelFormatType_422YpCbCr8_yuvs,
                kCVPixelFormatType_32BGRA,
                kCVPixelFormatType_32ARGB,
                kCVPixelFormatType_32RGBA,
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            ];

            let preferred_format_type = preferred_format_types
                .into_iter()
                .find(|format_type| {
                    available_format_types.containsObject(&NSNumber::new_u32(*format_type))
                })
                .ok_or(XCapError::new(
                    "no preferred pixel format type is supported by the output",
                ))?;

            log::info!(
                "preferred pixel format type 0x{preferred_format_type:08X} ({}) is supported and will be used",
                DataOutputSampleBufferDelegateVars::pixel_format_name(preferred_format_type)
            );

            let format_type_value = NSNumber::new_u32(preferred_format_type);

            // 创建 NSDictionary
            let video_settings: Retained<NSDictionary<NSString>> =
                NSDictionary::from_slices::<NSString>(
                    &[format_type_key.as_ref()],
                    &[format_type_value.as_ref()],
                );

            output.setVideoSettings(Some(&video_settings));

            if session.canAddOutput(&output) {
                session.addOutput(&output)
            }

            let (tx, rx) = sync_channel(0);
            let running = Arc::new(AtomicBool::new(false));

            let delegate = DataOutputSampleBufferDelegate::new(tx.clone(), running.clone());

            let sample_buffer_delegate = ProtocolObject::<
                dyn AVCaptureVideoDataOutputSampleBufferDelegate,
            >::from_ref(&*delegate);

            let queue =
                DispatchQueue::new("DataOutputSampleBufferDelegate", DispatchQueueAttr::SERIAL);

            let queue: &DispatchQueue = queue.as_ref();

            let _: () =
                msg_send![&output, setSampleBufferDelegate: sample_buffer_delegate, queue: queue];

            Ok((
                ImplVideoRecorder {
                    session,
                    _output: output,
                    _input: input,
                    _delegate: delegate,
                    running,
                },
                rx,
            ))
        }
    }

    pub fn start(&self) -> XCapResult<()> {
        self.running.store(true, Ordering::Release);
        unsafe { self.session.startRunning() };
        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.running.store(false, Ordering::Release);
        unsafe { self.session.stopRunning() };
        Ok(())
    }
}
