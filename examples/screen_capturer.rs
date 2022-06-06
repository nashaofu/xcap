use screen_capturer::ScreenCapturer;
use std::{fs::File, io::Write, time::Instant};

fn main() {
  let start = Instant::now();
  let screen_capturers = ScreenCapturer::all();

  for screen_capturer in screen_capturers {
    println!("capturer {:?}", screen_capturer);
    let image = screen_capturer.capture().unwrap();
    let buffer = image.png().unwrap();
    let display_id = screen_capturer.display_info.id.to_string();
    let path = String::from("") + &display_id + ".png";
    let mut file = File::create(path).unwrap();
    file.write_all(&buffer[..]).unwrap();
  }

  let screen_capturer = ScreenCapturer::from_point(100, 100).unwrap();
  println!("capturer {:?}", screen_capturer);

  let image = screen_capturer.capture().unwrap();
  let buffer = image.png().unwrap();
  let mut file = File::create("capture_display_with_point.png").unwrap();
  file.write_all(&buffer[..]).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}
