use std::{thread, time::Duration};
use xcap::Monitor;

fn main() {
    let monitor = Monitor::from_point(100, 100).unwrap();

    let (video_recorder, sx) = monitor.video_recorder().unwrap();

    thread::spawn(move || {
        loop {
            match sx.recv() {
                Ok(frame) => {
                    println!("frame: {:?}", frame.width);
                }
                _ => continue,
            }
        }
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
