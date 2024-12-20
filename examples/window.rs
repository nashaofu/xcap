use std::thread;
use xcap::Window;

fn main() {
    thread::sleep(std::time::Duration::from_secs(3));

    let windows = Window::all().unwrap();

    for window in windows.clone() {
        println!(
            "Window:\n id: {}\n title: {}\n app_name: {}\n pid: {}\n monitor: {:?}\n position: {:?}\n size {:?}\n state {:?}\n",
            window.id(),
            window.title(),
            window.app_name(),
            window.pid(),
            window.current_monitor().name(),
            (window.x(), window.y(), window.z()),
            (window.width(), window.height()),
            (window.is_minimized(), window.is_maximized())
        );
    }
}
