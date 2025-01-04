use fs_extra::dir;
use std::{
    thread,
    time::{Duration, Instant},
};
use xcap::Monitor;

fn normalized(filename: &str) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() {
    thread::sleep(Duration::from_secs(1));
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();

    dir::create_all("target/monitors", true).unwrap();

    for monitor in monitors {
        let image = monitor.capture_image().unwrap();

        image
            .save(format!(
                "target/monitors/monitor-{}.png",
                normalized(monitor.name())
            ))
            .unwrap();
    }

    println!("运行耗时: {:?}", start.elapsed());
}
