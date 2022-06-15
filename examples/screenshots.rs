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
