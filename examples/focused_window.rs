use std::thread;
use xcap::Window;

fn main() {
    thread::sleep(std::time::Duration::from_secs(3));

    let windows = Window::all().unwrap();

    loop {
        windows.iter().filter(|w| w.is_focused()).for_each(|focused| {
            println!(
                "Focused Window:\n id: {}\n title: {}\n app_name: {}\n monitor: {:?}\n position: {:?}\n size {:?}\n state {:?}\n",
                focused.id(),
                focused.title(),
                focused.app_name(),
                focused.current_monitor().name(),
                (focused.x(), focused.y(), focused.z()),
                (focused.width(), focused.height()),
                (focused.is_minimized(), focused.is_maximized(), focused.is_focused())
            );
        });

        thread::sleep(std::time::Duration::from_secs(1));
    }
}
