# XCap

English | [简体中文](README-zh_CN.md)

XCap is a cross-platform screen capture library written in Rust. It supports Linux (X11, Wayland), MacOS, and Windows. XCap supports screenshot and video recording (WIP).

## Features

-   Cross-platform: Supports Linux (X11, Wayland), MacOS, and Windows.
-   Supports multiple screenshot modes: Can take screenshots of the screen and windows.
-   Supports video recording: Supports recording of the screen or window (WIP).

### Implementation Status

| Feature          | Linux(X11) | Linux(Wayland) | MacOS | Windows(>=Windows 8.1) |
| ---------------- | ---------- | -------------- | ----- | ---------------------- |
| Screen Capture   | ✅         | ⛔             | ✅    | ✅                     |
| Window Capture   | ✅         | ⛔             | ✅    | ✅                     |
| Screen Recording | ✅         | ⛔             | ✅    | ✅                     |
| Window Recording | 🛠️         | 🛠️             | 🛠️    | 🛠️                     |

-   ✅: Feature available
-   ⛔: Feature available, but not fully supported in some special scenarios
-   🛠️: To be developed

## Examples

-   Screen Capture

```rust
use fs_extra::dir;
use std::time::Instant;
use xcap::Monitor;

fn normalized(filename: String) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() {
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();

    dir::create_all("target/monitors", true).unwrap();

    for monitor in monitors {
        let image = monitor.capture_image().unwrap();

        image
            .save(format!(
                "target/monitors/monitor-{}.png",
                normalized(monitor.name().unwrap())
            ))
            .unwrap();
    }

    println!("运行耗时: {:?}", start.elapsed());
}

```

-   Screen Record

```rust
use std::{thread, time::Duration};
use xcap::Monitor;

fn main() {
    let monitor = Monitor::from_point(100, 100).unwrap();

    let (video_recorder, sx) = monitor.video_recorder().unwrap();

    thread::spawn(move || loop {
        match sx.recv() {
            Ok(frame) => {
                println!("frame: {:?}", frame.width);
            }
            _ => continue,
        }
    });

    println!("start");
    video_recorder.start().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("stop");
    video_recorder.stop().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("start");
    video_recorder.start().unwrap();
    thread::sleep(Duration::from_secs(2));
    println!("stop");
    video_recorder.stop().unwrap();
}

```

-   Window Capture

```rust
use fs_extra::dir;
use std::time::Instant;
use xcap::Window;

fn normalized(filename: &str) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() {
    let start = Instant::now();
    let windows = Window::all().unwrap();

    dir::create_all("target/windows", true).unwrap();

    let mut i = 0;
    for window in windows {
        // 最小化的窗口不能截屏
        if window.is_minimized().unwrap() {
            continue;
        }

        println!(
            "Window: {:?} {:?} {:?}",
            window.title().unwrap(),
            (
                window.x().unwrap(),
                window.y().unwrap(),
                window.width().unwrap(),
                window.height().unwrap()
            ),
            (
                window.is_minimized().unwrap(),
                window.is_maximized().unwrap()
            )
        );

        let image = window.capture_image().unwrap();
        image
            .save(format!(
                "target/windows/window-{}-{}.png",
                i,
                normalized(&window.title().unwrap())
            ))
            .unwrap();

        i += 1;
    }

    println!("运行耗时: {:?}", start.elapsed());
}

```

More examples in [examples](./examples)

## Linux System Requirements

On Linux, the following dependencies need to be installed to compile properly.

Debian/Ubuntu:

```sh
apt-get install pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev
```

Alpine:

```sh
apk add pkgconf llvm19-dev clang19-dev libxcb-dev libxrandr-dev dbus-dev pipewire-dev
```

ArchLinux:

```sh
pacman -S base-devel clang libxcb libxrandr dbus libpipewire
```

## License

This project is licensed under the Apache License. See the [LICENSE](./LICENSE) file for details.
