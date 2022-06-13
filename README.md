# screenshots

A cross-platform screenshots library for MacOS、Windows、Linux(X11、wayland).

## example

```rust
use screenshots::Screenshots;
use std::{fs::File, io::Write, time::Instant};

fn main() {
  let start = Instant::now();

  let screenshotss = Screenshots::all();

  for screenshots in screenshotss {
    println!("capturer {:?}", screenshots);
    let image = screenshots.capture().unwrap();
    let buffer = image.png().unwrap();
    let display_id = screenshots.display_info.id.to_string();
    let path = String::from("") + &display_id + ".png";
    let mut file = File::create(path).unwrap();
    file.write_all(&buffer[..]).unwrap();
  }

  let screenshots = Screenshots::from_point(100, 100).unwrap();
  println!("capturer {:?}", screenshots);

  let image = screenshots.capture().unwrap();
  let buffer = image.png().unwrap();
  let mut file = File::create("capture_display_with_point.png").unwrap();
  file.write_all(&buffer[..]).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}

```
