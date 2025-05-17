use std::thread;
use xcap::Window;

fn main() {
    thread::sleep(std::time::Duration::from_secs(3));

    let windows = Window::all().unwrap();

    for window in windows.clone() {
        println!(
            "Window:\n id: {}\n pid: {}\n app_name: {}\n title: {}\n monitor: {}\n position: {:?}\n size {:?}\n state {:?}\n",
            window.id().unwrap(),
            window.pid().unwrap(),
            window.app_name().unwrap(),
            window.title().unwrap(),
            window.current_monitor().unwrap().name().unwrap(),
            (
                window.x().unwrap(),
                window.y().unwrap(),
                window.z().unwrap()
            ),
            (window.width().unwrap(), window.height().unwrap()),
            (
                window.is_minimized().unwrap(),
                window.is_maximized().unwrap(),
                window.is_focused().unwrap()
            )
        );
    }
}
