use crate::{DisplayInfo, Image};
use dbus::{self, blocking::Connection};
use std::{
  env::temp_dir,
  fs, io,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

fn screenshot(x: i32, y: i32, width: i32, height: i32) -> Result<String, dbus::Error> {
  let conn = Connection::new_session()?;

  let proxy = conn.with_proxy(
    "org.gnome.Shell.Screenshot",
    "/org/gnome/Shell/Screenshot",
    Duration::from_secs(10),
  );

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

pub fn wayland_capture_screen(display_info: &DisplayInfo) -> Option<Image> {
  let x = ((display_info.x as f32) * display_info.scale_factor) as i32;
  let y = ((display_info.y as f32) * display_info.scale_factor) as i32;
  let width = (display_info.width as f32) * display_info.scale_factor;
  let height = (display_info.height as f32) * display_info.scale_factor;

  let filename = screenshot(x, y, width as i32, height as i32).ok()?;
  let buffer = read_image(filename).ok()?;

  Some(Image::new(width as u32, height as u32, buffer))
}

pub fn wayland_capture_screen_area(
  display_info: &DisplayInfo,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  let area_x = (((x + display_info.x) as f32) * display_info.scale_factor) as i32;
  let area_y = (((y + display_info.y) as f32) * display_info.scale_factor) as i32;
  let area_width = (width as f32) * display_info.scale_factor;
  let area_height = (height as f32) * display_info.scale_factor;

  let filename = screenshot(area_x, area_y, area_width as i32, area_height as i32).ok()?;
  let buffer = read_image(filename).ok()?;

  Some(Image::new(width as u32, height as u32, buffer))
}
