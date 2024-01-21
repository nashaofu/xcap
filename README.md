# 📷 XCap

XCap is a cross-platform screen capture library for MacOS, Windows, Linux (X11, Wayland) written in Rust. It provides a simple API for capturing screen capture of a screen or a specific area of a screen.

## Features

-   Cross-platform support: Windows Mac and Linux.
-   Multiple capture modes: screen window.
-   Video capture、audio capture soon.

## Example

-   Monitor capture

```rust
use xcap::Monitor;
use std::time::Instant;

fn normalized(filename: &str) -> String {
    filename
        .replace("|", "")
        .replace("\\", "")
        .replace(":", "")
        .replace("/", "")
}

fn main() {
    let start = Instant::now();
    let monitors = Monitor::all().unwrap();

    for monitor in monitors {
        let image = monitor.capture_image().unwrap();

        image
            .save(format!("target/monitor-{}.png", normalized(monitor.name())))
            .unwrap();
    }

    println!("运行耗时: {:?}", start.elapsed());
}
```

-   Window capture

```rust
use xcap::Window;
use std::time::Instant;

fn normalized(filename: &str) -> String {
    filename
        .replace("|", "")
        .replace("\\", "")
        .replace(":", "")
        .replace("/", "")
}

fn main() {
    let start = Instant::now();
    let windows = Window::all().unwrap();

    let mut i = 0;

    for window in windows {
        // 最小化的窗口不能截屏
        if window.is_minimized() {
            continue;
        }

        println!(
            "Window: {:?} {:?} {:?}",
            window.title(),
            (window.x(), window.y(), window.width(), window.height()),
            (window.is_minimized(), window.is_maximized())
        );

        let image = window.capture_image().unwrap();
        image
            .save(format!(
                "target/window-{}-{}.png",
                i,
                normalized(window.title())
            ))
            .unwrap();

        i += 1;
    }

    println!("运行耗时: {:?}", start.elapsed());
}
```

## Linux Requirements

On Linux, you need to install `libxcb`, `libxrandr`, and `dbus`.

Debian/Ubuntu:

```sh
apt-get install libxcb1 libxrandr2 libdbus-1-3
```

Alpine:

```sh
apk add libxcb libxrandr dbus
```

ArchLinux:

```sh
pacman -S libxcb libxrandr dbus
```

## License

This project is licensed under the Apache License. See the [LICENSE](../LICENSE) file for details.
