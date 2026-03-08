use std::{
    thread,
    time::{Duration, Instant},
};
use xcap::Monitor;

fn main() {
    let start = Instant::now();
    let monitor = Monitor::from_point(100, 100).unwrap();

    let (video_recorder, sx) = monitor.video_recorder().unwrap();

    thread::spawn(move || {
        let mut prev = start.elapsed();
        loop {
            match sx.recv() {
                Ok(frame) => {
                    println!(
                        "frame: {:?}, elapsed: {:?}",
                        frame.width,
                        start.elapsed() - prev
                    );
                    prev = start.elapsed();
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
