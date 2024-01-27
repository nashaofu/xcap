use std::time::Instant;
use xcap::Window;

fn main() {
    let start = Instant::now();
    let windows = Window::all().unwrap();
    println!("Window::all() 运行耗时: {:?}", start.elapsed());

    for window in windows {
        println!(
            "Window:\n id: {}\n title: {}\n app_name: {}\n monitor: {:?}\n position: {:?}\n size {:?}\n state {:?}\n",
            window.id(),
            window.title(),
            window.app_name(),
            window.current_monitor().name(),
            (window.x(), window.y()),
            (window.width(), window.height()),
            (window.is_minimized(), window.is_maximized())
        );
    }

    println!("运行耗时: {:?}", start.elapsed());
}
