# screenshots

A cross-platform screenshots library for MacOS、Windows、Linux(X11、wayland).

## example

```rust
use screenshots::Screen;
use std::{fs, time::Instant};

fn main() {
  let start = Instant::now();
  let screens = Screen::all();

  for screen in screens {
    println!("capturer {:?}", screen);
    let mut image = screen.capture().unwrap();
    let mut buffer = image.buffer();
    fs::write(format!("target/{}.png", screen.id.to_string()), &buffer).unwrap();

    image = screen.capture_area(300, 300, 300, 300).unwrap();
    buffer = image.buffer();
    fs::write(format!("target/{}-2.png", screen.id.to_string()), &buffer).unwrap();
  }

  let screen = Screen::from_point(100, 100).unwrap();
  println!("capturer {:?}", screen);

  let image = screen.capture_area(300, 300, 300, 300).unwrap();
  let buffer = image.buffer();
  fs::write("target/capture_display_with_point.png", &buffer).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}

```

## Wayland

[Wayland doesn’t support third-party screenshots/capture](https://drewdevault.com/2019/02/10/Wayland-misconceptions-debunked.html), I refer to other screenshots or screen recording programs and implement the screenshot in the same way. However, there is a defect. Every time you take a screenshot, a system screenshot prompt will pop up, and you need to click share to complete the screenshot.

![screenshot](https://user-images.githubusercontent.com/19303058/191504323-a2d638ee-3612-4692-a72c-5189c58bcdf2.png)

screenshot prompt pop-up conditions: Wayland desktop environment and cannot call `org.gnome.Shell.Screenshot` failed.

issue: https://github.com/nashaofu/screenshots-rs/issues/18

## API

### `Screen`: Screen capturer

- `Screen::new(display_info)`: Get screen from [display info](https://docs.rs/display-info/latest/display_info/struct.DisplayInfo.html), return `Option<Screen>`.
- `Screen::all()`: Get all screen, return `Vec<Screen>`.
- `Screen::from_point(x, y)`: Get screen from point, return `Option<Screen>`.
- `screen.capture()`: capture screen screenshot [image](https://docs.rs/screenshots/latest/screenshots/struct.Image.html), return `Option<Image>`.
- `screen.capture_area(x, y, width, height)`: Capture the current screen the designated area, return `Option<Image>`.

### `Image`: Screen screenshot image

- `Image::new(width, height, buffer)`: Get image from width、height and rgba buffer, return `Image`.
- `Image::from_bgra(width, height, buffer)`: Get image from width、height and bgra buffer, return `Result<Image, EncodingError>`.
- `image.width()`: Get image width, return `u32`.
- `image.height()`: Get image height, return `u32`.
- `image.buffer()`: Get image buffer, return `Vec<u8>`.

## Linux requirements

On Linux, you need to install `libxcb`、`libxrandr`、`dbus`

Debian/Ubuntu:

```sh
apt-get install libxcb1 libxrandr2 libdbus-1-3
```

Alpine:

```sh
apk add libxcb libxrandr dbus
```
