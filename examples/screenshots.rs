use screenshots::Screen;
use std::{fs, time::Instant};

fn main() {
  let start = Instant::now();
  let screens = Screen::all().unwrap();

  for screen in screens {
    println!("capturer {:?}", screen);
    let mut image = screen.capture().unwrap();
    let mut buffer = image.buffer();
    fs::write(format!("target/{}.png", screen.display_info.id), &buffer).unwrap();

    image = screen.capture_area(300, 300, 300, 300).unwrap();
    buffer = image.buffer();
    fs::write(format!("target/{}-2.png", screen.display_info.id), &buffer).unwrap();
  }

  let screen = Screen::from_point(100, 100).unwrap();
  println!("capturer {:?}", screen);

  let image = screen.capture_area(300, 300, 300, 300).unwrap();
  let buffer = image.buffer();
  fs::write("target/capture_display_with_point.png", &buffer).unwrap();

  // Get the pixels of the image
  let pixels = image.pixels();
  for pixel in pixels {
      if pixel.r == 255 && pixel.g == 255 && pixel.b == 255 {
          println!("Found white pixel at x: {}, y: {}", pixel.x, pixel.y);
      }
  }

  // Get specific pixel
  let pixel = image.get_pixel(0, 0);
  println!("Pixel at x: {}, y: {} has rgb values: {}, {}, {}", pixel.x, pixel.y, pixel.r, pixel.g, pixel.b);

  println!("运行耗时: {:?}", start.elapsed());
}
