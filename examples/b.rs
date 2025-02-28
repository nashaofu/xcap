use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use dispatch2::{Queue, QueueAttribute};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, AllocAnyThread, DefinedClass,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureOutput, AVCaptureScreenInput, AVCaptureSession,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
};
use objc2_core_graphics::CGDirectDisplayID;
use objc2_core_media::{CMSampleBuffer, CMSampleBufferGetImageBuffer};
use objc2_foundation::{NSObject, NSObjectProtocol};

#[derive(Debug, Clone)]
struct Ivars {
    tx: Sender<u8>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "DataOutputSampleBufferDelegate"]
    #[ivars = Ivars]
    #[derive(Debug)]
    struct DataOutputSampleBufferDelegate;

    unsafe impl NSObjectProtocol for DataOutputSampleBufferDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for DataOutputSampleBufferDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            connection: &AVCaptureConnection,
        ) {
            let tx = self.ivars().tx.clone();
            tx.send(1).unwrap();
            let image_buffer = CMSampleBufferGetImageBuffer(sample_buffer).unwrap();
        }
    }
);

impl DataOutputSampleBufferDelegate {
    fn new(tx: Sender<u8>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { tx });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Debug)]
struct XCapError {
    message: String,
}

impl XCapError {
    pub fn new<S: ToString>(err: S) -> Self {
        XCapError {
            message: err.to_string(),
        }
    }
}

pub type XCapResult<T> = Result<T, XCapError>;

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {
    session: Arc<Mutex<AVCaptureSession>>,
    input: Arc<Mutex<AVCaptureScreenInput>>,
    output: Arc<Mutex<AVCaptureVideoDataOutput>>,
    delegate: Arc<Mutex<DataOutputSampleBufferDelegate>>,
    tx: Sender<u8>,
    rx: Arc<Mutex<Receiver<u8>>>,
}

impl ImplVideoRecorder {
    pub fn new(cg_direct_display_id: CGDirectDisplayID) -> XCapResult<Self> {
        unsafe {
            let session = AVCaptureSession::new();
            let input = AVCaptureScreenInput::initWithDisplayID(
                AVCaptureScreenInput::alloc(),
                cg_direct_display_id,
            )
            .ok_or(XCapError::new(
                "AVCaptureScreenInput::initWithDisplayID failed",
            ))?;

            if session.canAddInput(&input) {
                session.addInput(&input);
            }

            let output = AVCaptureVideoDataOutput::new();

            if session.canAddOutput(&output) {
                session.addOutput(&output)
            }

            let (tx, rx) = channel();

            let delegate = DataOutputSampleBufferDelegate::new(tx.clone());

            let sample_buffer_delegate = ProtocolObject::<
                dyn AVCaptureVideoDataOutputSampleBufferDelegate,
            >::from_ref(&*delegate);

            let queue = Queue::new("DataOutputSampleBufferDelegate", QueueAttribute::Concurrent);

            let _: () = msg_send![&output, setSampleBufferDelegate: sample_buffer_delegate, queue: queue.as_raw()];

            Ok(ImplVideoRecorder {
                session: Arc::new(Mutex::new(*session)),
                output: Arc::new(Mutex::new(*output)),
                input: Arc::new(Mutex::new(*input)),
                delegate: Arc::new(Mutex::new(*delegate)),
                tx,
                rx: Arc::new(Mutex::new(rx)),
            })
        }
    }

    pub fn on_frame<F>(&self, on_frame: F) -> XCapResult<()>
    where
        F: Fn(u8) -> XCapResult<()> + Send + 'static,
    {
        let rx = self.rx.lock().unwrap();

        loop {
            match rx.recv() {
                Ok(frame) => {
                    on_frame(frame)?;
                }
                Err(err) => break Err(XCapError::new(err)),
            }
        }
    }

    pub fn start(&self) -> XCapResult<()> {
        let session = self.session.lock().unwrap();
        unsafe { session.startRunning() };
        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        let session = self.session.lock().unwrap();
        unsafe { session.stopRunning() };
        Ok(())
    }
}

fn main() {
    let video_recorder = Arc::new(ImplVideoRecorder::new(1).unwrap());
    let video_recorder_clone = video_recorder.clone();

    thread::spawn(move || {
        video_recorder_clone
            .on_frame(|frame| {
                println!("frame: {:?}", frame);
                Ok(())
            })
            .unwrap();
    });

    video_recorder.start().unwrap();
    thread::sleep(Duration::from_secs(2));
}
