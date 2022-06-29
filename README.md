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
    let image = screen.capture().unwrap();
    let buffer = image.buffer();
    fs::write(format!("{}.png", screen.id.to_string()), &buffer).unwrap();
  }

  let screen = Screen::from_point(100, 100).unwrap();
  println!("capturer {:?}", screen);

  let image = screen.capture().unwrap();
  let buffer = image.buffer();
  fs::write("capture_display_with_point.png", &buffer).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}

```

## API

### `Screen`: Screen capturer

- `Screen::all()`: Get all screen, return `Vec<Screen>`.
- `Screen::from_point(x, y)`: Get screen from point, return `Option<Screen>`.
- `Screen::new(display_info)`: Get screen from [display info](https://docs.rs/display-info/latest/display_info/struct.DisplayInfo.html), return `Option<Screen>`.
- `screen.capture()`: capture screen screenshot [image](https://docs.rs/screenshots/latest/screenshots/struct.Image.html), return `Option<Image>`.
