use std::time::Instant;
use xcap::Monitor;

fn normalized(filename: &str) -> String {
    filename
        .replace("|", "")
        .replace("\\", "")
        .replace(":", "")
        .replace("/", "")
}

fn main() {
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();

    for monitor in monitors {
        let image = monitor.capture_image().unwrap();

        image
            .save(format!("target/monitor-{}.png", normalized(monitor.name())))
            .unwrap();
    }

    println!("运行耗时: {:?}", start.elapsed());
}
