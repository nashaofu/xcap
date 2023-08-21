# 📷 Screenshots

Screenshots is a cross-platform screenshots library for MacOS, Windows, Linux (X11, Wayland) written in Rust. It provides a simple API for capturing screenshots of a screen or a specific area of a screen.

## Example

The following example shows how to capture screenshots of all screens and a specific area of a screen.

```rust
use screenshots::Screen;
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let screens = Screen::all().unwrap();

    for screen in screens {
        println!("capturer {screen:?}");
        let mut image = screen.capture().unwrap();
        image
            .save(format!("target/{}.png", screen.display_info.id))
            .unwrap();

        image = screen.capture_area(300, 300, 300, 300).unwrap();
        image
            .save(format!("target/{}-2.png", screen.display_info.id))
            .unwrap();
    }

    let screen = Screen::from_point(100, 100).unwrap();
    println!("capturer {screen:?}");

    let image = screen.capture_area(300, 300, 300, 300).unwrap();
    image.save("target/capture_display_with_point.png").unwrap();
    println!("运行耗时: {:?}", start.elapsed());
}
```

## API

### `Screen`

The `Screen` struct represents a screen capturer and provides the following methods:

- `Screen::new(display_info)`: Get a screen from the [display info](https://docs.rs/display-info/latest/display_info/struct.DisplayInfo.html), returns a `Screen`.
- `Screen::all()`: Get all screens, returns `Result<Vec<Screen>>`.
- `Screen::from_point(x, y)`: Get a screen from a point, returns `Result<Screen>`.
- `screen.capture()`: Capture a screenshot of the screen, returns a [image](https://docs.rs/image/latest/image/type.RgbaImage.html) as `Result<RgbaImage>`.
- `screen.capture_area(x, y, width, height)`: Capture a screenshot of the designated area of the screen, returns the same as `capture()`.

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
