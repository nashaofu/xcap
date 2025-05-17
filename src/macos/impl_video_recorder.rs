use std::{
    slice,
    sync::mpsc::{Receiver, SyncSender, sync_channel},
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
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetDataSize,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use scopeguard::defer;

use crate::{XCapError, XCapResult, video_recorder::Frame};

#[derive(Debug, Clone)]
struct DataOutputSampleBufferDelegateVars {
    tx: SyncSender<Frame>,
}

impl DataOutputSampleBufferDelegateVars {
    fn capture(
        &self,
        _output: &AVCaptureOutput,
        sample_buffer: &CMSampleBuffer,
        _connection: &AVCaptureConnection,
    ) {
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

            if format_type != kCVPixelFormatType_32BGRA {
                log::error!("pixel format type {format_type} is not supported");
                return;
            }

            let width = CVPixelBufferGetWidth(&pixel_buffer);
            let height = CVPixelBufferGetHeight(&pixel_buffer);
            let bytes_per_row = CVPixelBufferGetBytesPerRow(&pixel_buffer);
            let base_address = CVPixelBufferGetBaseAddress(&pixel_buffer);
            let size = CVPixelBufferGetDataSize(&pixel_buffer);
            let data = slice::from_raw_parts(base_address as *mut u8, size);

            let mut buffer = Vec::with_capacity(width * height * 4);
            for row in data.chunks_exact(bytes_per_row) {
                buffer.extend_from_slice(&row[..width * 4]);
            }

            for bgra in buffer.chunks_exact_mut(4) {
                bgra.swap(0, 2);
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
    fn new(tx: SyncSender<Frame>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DataOutputSampleBufferDelegateVars { tx });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {
    session: Retained<AVCaptureSession>,
    _input: Retained<AVCaptureScreenInput>,
    _output: Retained<AVCaptureVideoDataOutput>,
    _delegate: Retained<DataOutputSampleBufferDelegate>,
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

            if session.canAddInput(&input) {
                session.addInput(&input);
            }

            let output = AVCaptureVideoDataOutput::new();
            output.setAlwaysDiscardsLateVideoFrames(true);
            output.setAutomaticallyConfiguresOutputBufferDimensions(true);

            let format_type_key =
                NSString::from_str(kCVPixelBufferPixelFormatTypeKey.to_string().as_str());
            // 创建 NSNumber
            let format_type_value = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
            let available_format_types = output.availableVideoCVPixelFormatTypes();
            if !available_format_types.containsObject(&format_type_value) {
                return Err(XCapError::new(
                    "kCVPixelFormatType_32BGRA is not supported ",
                ));
            }

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

            let delegate = DataOutputSampleBufferDelegate::new(tx.clone());

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
                },
                rx,
            ))
        }
    }

    pub fn start(&self) -> XCapResult<()> {
        unsafe { self.session.startRunning() };
        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        unsafe { self.session.stopRunning() };
        Ok(())
    }
}
