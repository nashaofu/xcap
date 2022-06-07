use crate::{Image, ScreenCapturer};
use dbus::{blocking::Connection, Error};
use std::{
  env::temp_dir,
  fs,
  time::{Duration, SystemTime},
};

fn screenshot(screen_capturer: &ScreenCapturer) -> Result<String, Error> {
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

  let path = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
    Ok(duration) => {
      let time_string = duration.as_micros().to_string();
      temp_dir().join(String::from("screenshot-") + &time_string + ".png")
    }
    Err(_) => return Err(Error::new_failed("org.gnome.Shell.Screenshot failed")),
  };

  let filename = match path.to_str() {
    Some(filename) => filename,
    None => return Err(Error::new_failed("org.gnome.Shell.Screenshot failed")),
  };

  let _: () = proxy.method_call(
    "org.gnome.Shell.Screenshot",
    "ScreenshotArea",
    (x, y, width, height, false, filename),
  )?;

  Ok(String::from(filename))
}

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  let file = match screenshot(&screen_capturer) {
    Ok(file) => file,
    Err(_) => return None,
  };

  let display_info = screen_capturer.display_info;
  let width = ((display_info.width as f32) * display_info.scale) as u32;
  let height = ((display_info.height as f32) * display_info.scale) as u32;

  match fs::read(file) {
    Ok(buffer) => Some(Image::new(width, height, buffer)),
    Err(_) => None,
  }
}
