use fs_extra::dir;
use std::{
    thread,
    time::{Duration, Instant},
};
use xcap::Monitor;

fn main() {
    let monitors = Monitor::all().unwrap();

    dir::create_all("target/monitors", true).unwrap();

    let monitor = monitors.get(0).unwrap().clone();

    let mut i = 0;
    let frame = 20;
    let start = Instant::now();
    let fps = 1000 / frame;

    loop {
        i += 1;
        let time = Instant::now();
        let image = monitor.capture_image().unwrap();
        image
            .save(format!("target/monitors/monitor-{}.png", i,))
            .unwrap();
        let sleep_time = fps * i - start.elapsed().as_millis() as i128;
        println!(
            "sleep_time: {:?} current_step_time: {:?}",
            sleep_time,
            time.elapsed()
        );
        if sleep_time > 0 {
            thread::sleep(Duration::from_millis(sleep_time as u64));
        }

        if i >= 900 {
            break;
        }
    }

    println!("time {:?}", start.elapsed());
    let actual_fps = 900 / start.elapsed().as_secs();
    println!("actual fps: {}", actual_fps);

    // ffmpeg -framerate {actual_fps} -i monitor-%d.png -c:v libx264 -pix_fmt yuv420p output.mp4
}
