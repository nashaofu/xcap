# XCap

[English](README.md) | 简体中文

XCap 是一个使用 Rust 编写的跨平台的屏幕捕获库，它支持 Linux(X11,Wayland)、MacOS 与 Windows。XCap 支持截图与视频录制（实现中）。

## 功能

-   跨平台: 支持 Linux(X11,Wayland)、MacOS 与 Windows。
-   支持多种截图模式: 可以对屏幕与窗口进行截图。
-   支持视频录制：支持对屏幕或窗口进行录制（实现中）。

### 实现状态

| 功能     | Linux(X11) | Linux(Wayland) | MacOS | Windows(>=Windows 8.1) |
| -------- | ---------- | -------------- | ----- | ---------------------- |
| 屏幕截图 | ✅         | ⛔             | ✅    | ✅                     |
| 窗口截图 | ✅         | ⛔             | ✅    | ✅                     |
| 屏幕录制 | ✅         | ⛔             | ✅    | ✅                     |
| 窗口录制 | 🛠️         | 🛠️             | 🛠️    | 🛠️                     |

-   ✅: 功能可用
-   ⛔: 功能可用，但在一些特殊场景下未完全支持
-   🛠️: 待开发

## 例子

-   屏幕截图

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
                normalized(monitor.friendly_name().unwrap())
            ))
            .unwrap();
    }

    println!("运行耗时: {:?}", start.elapsed());
}

```

- 屏幕区域截图

```rust
use fs_extra::dir;
use std::time::Instant;
use xcap::Monitor;

fn normalized(filename: String) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitors = Monitor::all()?;
    dir::create_all("target/monitors", true).unwrap();

    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .expect("No primary monitor found");

    let monitor_width = monitor.width()?;
    let monitor_height = monitor.height()?;

    let region_width = 400u32;
    let region_height = 300u32;

    let x = ((monitor_width as i32) - (region_width as i32)) / 2;
    let y = ((monitor_height as i32) - (region_height as i32)) / 2;
    let start = Instant::now();

    let image = monitor.capture_region(x, y, region_width, region_height)?;
    println!(
        "Time to record region of size {}x{}: {:?}",
        image.width(),
        image.height(),
        start.elapsed()
    );

    image
        .save(format!(
            "target/monitors/monitor-{}-region.png",
            normalized(monitor.friendly_name().unwrap())
        ))
        .unwrap();

    Ok(())
}

```

-   屏幕录制

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

-   窗口截图

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

更多例子可以在 [examples](./examples) 目录中找到。

## Linux 系统要求

在 Linux 上，需要安装下面这些依赖才能正常编译。

Debian/Ubuntu:

```sh
apt-get install pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev
```

Alpine:

```sh
apk add pkgconf llvm19-dev clang19-dev libxcb-dev libxrandr-dev dbus-dev pipewire-dev wayland-dev mesa-dev
```

ArchLinux:

```sh
pacman -S base-devel clang libxcb libxrandr dbus libpipewire
```

## License

本项目采用 Apache 许可证。详情请查看 [LICENSE](./LICENSE) 文件。
