use std::{sync::Arc, thread, time::Duration};
use xcap::Monitor;

fn main() {
    let monitor = Monitor::from_point(100, 100).unwrap();

    let video_recorder = Arc::new(monitor.video_recorder().unwrap());

    let video_recorder_clone = video_recorder.clone();
    thread::spawn(move || {
        video_recorder_clone
            .on_frame(|frame| {
                println!("frame: {:?}", frame.width);
                Ok(())
            })
            .unwrap();
    });

    println!("start");
    video_recorder.start().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("stop");
    video_recorder.stop().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("start");
    video_recorder.start().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("stop");
    video_recorder.stop().unwrap();
}
