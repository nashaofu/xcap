use std::{thread, time::Duration};

use objc2::{
    define_class, extern_methods, msg_send, rc::Allocated, rc::Retained, runtime::ProtocolObject,
    AllocAnyThread, Encoding, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureFileOutput, AVCaptureFileOutputRecordingDelegate,
    AVCaptureMovieFileOutput, AVCaptureScreenInput, AVCaptureSession, AVCaptureVideoDataOutput,
};
use objc2_foundation::{
    ns_string, NSArray, NSCopying, NSError, NSObject, NSObjectProtocol, NSString, NSURL,
};
use xcap::Monitor;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyAppDelegate"]
    #[thread_kind = MainThreadOnly]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl AVCaptureFileOutputRecordingDelegate for AppDelegate {}
);

impl AppDelegate {
    extern_methods!(
        #[unsafe(method(init))]
        #[unsafe(method_family = init)]
        pub unsafe fn init(this: Allocated<Self>) -> Retained<Self>;

        #[unsafe(method(new))]
        #[unsafe(method_family = new)]
        pub unsafe fn new() -> Retained<Self>;
    );
}

fn main() {
    unsafe {
        let session = AVCaptureSession::new();
        // let output = AVCaptureVideoDataOutput::sampleBufferDelegate(&self)
        let output = AVCaptureMovieFileOutput::new();
        let monitor = Monitor::from_point(100, 100).unwrap();
        let input = AVCaptureScreenInput::initWithDisplayID(
            AVCaptureScreenInput::alloc(),
            monitor.id().unwrap(),
        )
        .unwrap();

        if session.canAddInput(&input) {
            session.addInput(&input);
        }

        if session.canAddOutput(&output) {
            session.addOutput(&output)
        }

        session.startRunning();
        let delegate = AppDelegate::new();
        let object = ProtocolObject::from_ref(&*delegate);

        output.startRecordingToOutputFileURL_recordingDelegate(
            &NSURL::fileURLWithPath(&NSString::from_str("./a.mp4")),
            object,
        );

        thread::sleep(Duration::from_secs(7));
        output.stopRecording();
        session.stopRunning();
        thread::sleep(Duration::from_secs(3));
    }
}
