use std::{
    thread,
    time::{Duration, Instant},
};
use xcap::{Monitor, image::RgbaImage};

fn main() {
    let start = Instant::now();
    let monitor = Monitor::from_point(100, 100).unwrap();

    let (video_recorder, sx) = monitor.video_recorder().unwrap();

    thread::spawn(move || {
        let mut prev = start.elapsed();
        let mut saved_first_frame = false;
        loop {
            match sx.recv() {
                Ok(frame) => {
                    println!(
                        "frame: {:?}, elapsed: {:?}",
                        frame.width,
                        start.elapsed() - prev
                    );
                    prev = start.elapsed();
                    if !saved_first_frame {
                        let image = RgbaImage::from_raw(frame.width, frame.height, frame.raw)
                            .expect("failed to create image from frame");
                        image
                            .save("target/monitor_recorded.png")
                            .expect("failed to save first frame");
                        println!("saved first frame: target/monitor_recorded.png");
                        saved_first_frame = true;
                    }
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
