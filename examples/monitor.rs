use std::time::Instant;
use xcap::Monitor;

fn main() {
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();
    println!("Monitor::all() 运行耗时: {:?}", start.elapsed());

    for monitor in monitors {
        println!(
            "Monitor:\n id: {}\n name: {}\n position: {:?}\n size: {:?}\n state:{:?}\n",
            monitor.id().unwrap(),
            monitor.name().unwrap(),
            (monitor.x().unwrap(), monitor.y().unwrap()),
            (monitor.width().unwrap(), monitor.height().unwrap()),
            (
                monitor.rotation().unwrap(),
                monitor.scale_factor().unwrap(),
                monitor.frequency().unwrap(),
                monitor.is_primary().unwrap(),
                monitor.is_builtin().unwrap()
            )
        );
    }

    let monitor = Monitor::from_point(100, 100).unwrap();

    println!("Monitor::from_point(): {:?}", monitor.name().unwrap());
    println!(
        "Monitor::from_point(100, 100) 运行耗时: {:?}",
        start.elapsed()
    );

    println!("运行耗时: {:?}", start.elapsed());
}
