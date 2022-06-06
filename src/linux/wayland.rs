use crate::{Image, ScreenCapturer};
use dbus::{blocking::Connection, Error};
use png::{Decoder, DecodingError};
use std::env::temp_dir;
use std::{fs::File, time::Duration};

fn screenshot(screen_capturer: &ScreenCapturer) -> Result<File, Error> {
  let conn = Connection::new_session()?;

  let proxy = conn.with_proxy(
    "org.gnome.Shell.Screenshot",
    "/org/gnome/Shell/Screenshot",
    Duration::from_secs(20),
  );

  let display_info = screen_capturer.display_info;
  let x = ((display_info.x as f32) * display_info.scale) as i32;
  let y = ((display_info.y as f32) * display_info.scale) as i32;
  let width = ((display_info.width as f32) * display_info.scale) as i32;
  let height = ((display_info.height as f32) * display_info.scale) as i32;
  let path = temp_dir().join("screenshot.png");
  let filename = match path.to_str() {
    Some(filename) => filename,
    None => return Err(Error::new_failed("org.gnome.Shell.Screenshot failed")),
  };

  let _: () = proxy.method_call(
    "org.gnome.Shell.Screenshot",
    "ScreenshotArea",
    (x, y, width, height, false, filename),
  )?;

  match File::open(path) {
    Ok(file) => Ok(file),
    Err(_) => Err(Error::new_failed("org.gnome.Shell.Screenshot failed")),
  }
}

fn file_to_bytes(file: File) -> Result<Vec<u8>, DecodingError> {
  let decoder = Decoder::new(file);

  let mut reader = decoder.read_info()?;
  // Allocate the output buffer.
  let mut buf = vec![0; reader.output_buffer_size()];
  // Read the next frame. An APNG might contain multiple frames.
  let info = reader.next_frame(&mut buf)?;
  let capacity = info.buffer_size();
  // Grab the bytes of the image.
  let mut bytes = vec![0; capacity];

  // RGB 转换为 BGR
  for i in (0..capacity).step_by(4) {
    let b = buf[i];
    let r = buf[i + 2];

    bytes[i] = r;
    bytes[i + 1] = buf[i + 1];
    bytes[i + 2] = b;
    bytes[i + 3] = 255;
  }

  Ok(bytes)
}

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  let file = match screenshot(&screen_capturer) {
    Ok(file) => file,
    Err(_) => return None,
  };

  let display_info = screen_capturer.display_info;
  let width = ((display_info.width as f32) * display_info.scale) as u32;
  let height = ((display_info.height as f32) * display_info.scale) as u32;

  match file_to_bytes(file) {
    Ok(bytes) => Some(Image {
      width,
      height,
      bytes,
    }),
    Err(_) => None,
  }
}
