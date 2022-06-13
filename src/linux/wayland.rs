use crate::{Image, Screenshots};
use dbus::{self, blocking::Connection};
use std::{
  env::temp_dir,
  fs, io,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

fn screenshot(screenshots: &Screenshots) -> Result<String, dbus::Error> {
  let conn = Connection::new_session()?;

  let proxy = conn.with_proxy(
    "org.gnome.Shell.Screenshot",
    "/org/gnome/Shell/Screenshot",
    Duration::from_secs(10),
  );

  let display_info = screenshots.display_info;
  let x = ((display_info.x as f32) * display_info.scale) as i32;
  let y = ((display_info.y as f32) * display_info.scale) as i32;
  let width = ((display_info.width as f32) * display_info.scale) as i32;
  let height = ((display_info.height as f32) * display_info.scale) as i32;

  let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
    Ok(duration) => duration.as_micros().to_string(),
    Err(_) => return Err(dbus::Error::new_failed("Get system timestamp failed")),
  };

  let dirname = temp_dir().join("screenshot");

  fs::create_dir_all(&dirname)
    .map_err(|_| dbus::Error::new_failed(format!("Create dir {:?} failed", dirname).as_str()))?;

  let mut path = dirname.join(timestamp);
  path.set_extension("png");

  let filename = path.to_string_lossy().to_string();

  let _: () = proxy.method_call(
    "org.gnome.Shell.Screenshot",
    "ScreenshotArea",
    (x, y, width, height, false, &filename),
  )?;

  Ok(filename)
}

fn read_image(filename: String) -> Result<Vec<u8>, io::Error> {
  let buffer = fs::read(&filename)?;
  fs::remove_file(&filename)?;
  Ok(buffer)
}

pub fn wayland_capture_display(screenshots: &Screenshots) -> Option<Image> {
  let filename = match screenshot(&screenshots) {
    Ok(file) => file,
    Err(_) => return None,
  };

  let display_info = screenshots.display_info;
  let width = ((display_info.width as f32) * display_info.scale) as u32;
  let height = ((display_info.height as f32) * display_info.scale) as u32;

  match read_image(filename) {
    Ok(buffer) => Some(Image::new(width, height, buffer)),
    Err(_) => None,
  }
}
