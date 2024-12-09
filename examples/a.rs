use std::time::Instant;
use xcap::Monitor;

fn main() {
    let start = Instant::now();
    let monitor = Monitor::from_point(100, 100).unwrap();

    monitor.start(|frame| {
        println!("{:?}", frame.width);
        Ok(())
    }).unwrap();

    println!("运行耗时: {:?}", start.elapsed());
}
