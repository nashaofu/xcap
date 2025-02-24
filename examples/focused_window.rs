use std::thread;
use xcap::Window;

fn main() {
    thread::sleep(std::time::Duration::from_secs(3));

    let windows = Window::all().unwrap();

    loop {
        windows.iter().filter(|w| w.is_focused().unwrap()).for_each(|focused| {
            println!(
                "Focused Window:\n id: {}\n title: {}\n app_name: {}\n monitor: {:?}\n position: {:?}\n size {:?}\n state {:?}\n",
                focused.id().unwrap(),
                focused.title().unwrap(),
                focused.app_name().unwrap(),
                focused.current_monitor().unwrap().name().unwrap(),
                (focused.x().unwrap(), focused.y().unwrap(), focused.z().unwrap()),
                (focused.width().unwrap(), focused.height().unwrap()),
                (focused.is_minimized().unwrap(), focused.is_maximized().unwrap(), focused.is_focused().unwrap())
            );
        });

        thread::sleep(std::time::Duration::from_secs(1));
    }
}
