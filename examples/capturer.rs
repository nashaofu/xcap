use screenshots::Capturer;
use std::{fs::File, io::Write, time::Instant};

fn main() {
  let start = Instant::now();

  let capturers = Capturer::screen_capturers();

  for capturer in capturers {
    println!("capturer {:?}", capturer);
    let image = capturer.capture_screen().unwrap();
    let buffer = image.png();
    let display_id = capturer.display_info.id.to_string();
    let path = String::from("") + &display_id + ".png";
    let mut file = File::create(path).unwrap();
    file.write_all(&buffer[..]).unwrap();
  }

  let capturer = Capturer::screen_capturer_from_point(100, 100).unwrap();
  println!("capturer {:?}", capturer);

  let image = capturer.capture_screen().unwrap();
  let buffer = image.png();
  let mut file = File::create("capture_display_with_point.png").unwrap();
  file.write_all(&buffer[..]).unwrap();

  println!("运行耗时: {:?}", start.elapsed());
}
