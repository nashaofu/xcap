use std::{sync::mpsc::channel, thread, time::Duration};

use block2::{Block, BlockFn, RcBlock, StackBlock};
use objc2::{
    rc::{Allocated, Retained},
    AllocAnyThread, Encode, Message,
};
use objc2_av_foundation::{AVVideoCodecType, AVVideoCodecTypeH264};
use objc2_core_media::{CMTime, CMTimeMake};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCRecordingOutput, SCRecordingOutputConfiguration,
    SCRecordingOutputDelegate, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamConfigurationPreset, SCStreamDelegate, SCStreamOutput, SCWindow,
};

fn main() {
    unsafe {
        let (tx, rx) = channel::<Retained<SCShareableContent>>();
        let callback = StackBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                if !error.is_null() {
                    let error: &NSError = (*error).as_ref();
                    println!("Error: {:?}", error);
                    return;
                }

                let content = (*content).retain();
                tx.send(content).unwrap();
            },
        );

        SCShareableContent::getShareableContentWithCompletionHandler(&callback);
        let content = rx.recv().unwrap();

        let displays = content.displays();
        let windows = content.windows();
        let apps = content.applications();
        for display in displays {
            println!("Display: {:?}", display);
        }
        for window in windows {
            println!("window: {:?}", window.title());
        }
        for app in apps {
            println!("app: {:?}", app);
        }

        let displays = content.displays();
        let display = displays.firstObject().unwrap().retain();
        let display: &SCDisplay = display.as_ref();

        let filter = SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            display,
            &NSArray::new(),
        );

        let config = SCStreamConfiguration::streamConfigurationWithPreset(
            SCStreamConfigurationPreset::CaptureHDRScreenshotLocalDisplay,
        );

        config.setMinimumFrameInterval(CMTimeMake(1, 1));
        let stream = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            None,
        );

        let recording_output = SCRecordingOutput::initWithConfiguration_delegate(
            SCRecordingOutput::alloc(),
            &output_config,
        );
        stream.addRecordingOutput_error(&recording_output).unwrap();

        stream.startCaptureWithCompletionHandler(None);
    }
}
