use std::time::Instant;
use xcap::Monitor;

fn main() {
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();
    println!("Monitor::all() 运行耗时: {:?}", start.elapsed());

    for monitor in monitors {
        println!(
            "Monitor: {} {} {:?} {:?}",
            monitor.id(),
            monitor.name(),
            (monitor.x(), monitor.y(), monitor.width(), monitor.height()),
            (
                monitor.rotation(),
                monitor.scale_factor(),
                monitor.frequency(),
                monitor.is_primary()
            )
        );
    }

    let monitor = Monitor::from_point(100, 100).unwrap();

    println!("Monitor::from_point(): {}", monitor.name());
    println!(
        "Monitor::from_point(100, 100) 运行耗时: {:?}",
        start.elapsed()
    );

    println!("运行耗时: {:?}", start.elapsed());
}
