# 📷 Screenshots

Screenshots is a cross-platform screenshots library for MacOS, Windows, Linux (X11, Wayland) written in Rust. It provides a simple API for capturing screenshots of a screen or a specific area of a screen.

## Example

The following example shows how to capture screenshots of all screens and a specific area of a screen.

```rust
use screenshots::{Screen, Compression};
use std::{fs, time::Instant};

fn main() {
  let start = Instant::now();
  let screens = Screen::all().unwrap();

  for screen in screens {
    println!("capturer {screen:?}");
    let mut image = screen.capture().unwrap();
    let mut buffer = image.to_png(Compression::Best).unwrap();
    fs::write(format!("target/{}.png", screen.display_info.id), buffer).unwrap();

    image = screen.capture_area(300, 300, 300, 300).unwrap();
    buffer = image.to_png(None).unwrap();
    fs::write(format!("target/{}-2.png", screen.display_info.id), buffer).unwrap();
  }

  let screen = Screen::from_point(100, 100).unwrap();
  println!("capturer {screen:?}");

  let image = screen.capture_area(300, 300, 300, 300).unwrap();
  let buffer = image.to_png(Compression::Fast).unwrap();
  fs::write("target/capture_display_with_point.png", buffer).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}
```

## API

### `Screen`

The `Screen` struct represents a screen capturer and provides the following methods:

- `Screen::new(display_info)`: Get a screen from the [display info](https://docs.rs/display-info/latest/display_info/struct.DisplayInfo.html), returns a `Screen`.
- `Screen::all()`: Get all screens, returns `Result<Vec<Screen>>`.
- `Screen::from_point(x, y)`: Get a screen from a point, returns `Result<Screen>`.
- `screen.capture()`: Capture a screenshot of the screen, returns a [image](https://docs.rs/screenshots/latest/screenshots/struct.Image.html).
- `screen.capture_area(x, y, width, height)`: Capture a screenshot of the designated area of the screen, returns a `Result<Image>`.

### `Image`

The `Image` struct represents a screen screenshot image and provides the following methods:

- `Image::new(width, height, rgba)`: Get an image from the width, height, and RGBA buffer, returns an `Image`.
- `Image::from_bgra(bgra, width, height, bytes_per_row)`: Get an image from BGRA buffer, returns `Image`.
- `image.width()`: Get the image width, returns `u32`.
- `image.height()`: Get the image height, returns `u32`.
- `image.rgba()`: Get the image rgba buffer, returns `Vec<u8>`.
- `image.to_png()`: Convert to png format buffer, returns `Result<Vec<u8>, EncodingError>`.

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

## License

This project is licensed under the Apache License. See the [LICENSE](LICENSE) file for details.
