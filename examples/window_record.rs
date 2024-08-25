use fs_extra::dir;
use std::{
    thread,
    time::{Duration, Instant},
};
use xcap::Window;

fn main() {
    let windows = Window::all().unwrap();

    dir::create_all("target/windows", true).unwrap();

    let mut i = 0;
    for window in &windows {
        // 最小化的窗口不能截屏
        if window.is_minimized() {
            continue;
        }

        if window.title().contains("Chrome") {
            break;
        }

        println!(
            "Window: {:?} {:?} {:?}",
            window.title(),
            (window.x(), window.y(), window.width(), window.height()),
            (window.is_minimized(), window.is_maximized())
        );

        i += 1;
    }

    let mut win = windows.get(i).unwrap().clone();
    println!("{:?}", win);

    let mut i = 0;
    let frame = 20;
    let start = Instant::now();
    let fps = 1000 / frame;

    loop {
        i += 1;
        let time = Instant::now();
        win.refresh().unwrap();
        let image = win.capture_image().unwrap();
        image
            .save(format!("target/windows/window-{}.png", i,))
            .unwrap();
        let sleep_time = fps * i - start.elapsed().as_millis() as i128;
        println!(
            "sleep_time: {:?} current_step_time: {:?}",
            sleep_time,
            time.elapsed()
        );
        if sleep_time > 0 {
            thread::sleep(Duration::from_millis(sleep_time as u64));
        }

        if i >= 900 {
            break;
        }
    }

    println!("time {:?}", start.elapsed());
    let actual_fps = 900 / start.elapsed().as_secs();
    println!("actual fps: {}", actual_fps);

    // ffmpeg -framerate {actual_fps} -i window-%d.png -c:v libx264 -pix_fmt yuv420p output.mp4
}
