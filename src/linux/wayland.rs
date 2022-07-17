use crate::{Image, Screen};
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

pub fn wayland_capture_screen(screen: &Screen) -> Option<Image> {
  let x = ((screen.x as f32) * screen.scale_factor) as i32;
  let y = ((screen.y as f32) * screen.scale_factor) as i32;
  let width = (screen.width as f32) * screen.scale_factor;
  let height = (screen.height as f32) * screen.scale_factor;

  let filename = match screenshot(x, y, width as i32, height as i32) {
    Ok(file) => file,
    Err(_) => return None,
  };

  match read_image(filename) {
    Ok(buffer) => Some(Image::new(width as u32, height as u32, buffer)),
    Err(_) => None,
  }
}

pub fn wayland_capture_screen_area(
  screen: &Screen,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  let area_x = (((x + screen.x) as f32) * screen.scale_factor) as i32;
  let area_y = (((y + screen.y) as f32) * screen.scale_factor) as i32;
  let area_width = (width as f32) * screen.scale_factor;
  let area_height = (height as f32) * screen.scale_factor;

  let filename = match screenshot(area_x, area_y, area_width as i32, area_height as i32) {
    Ok(file) => file,
    Err(_) => return None,
  };

  match read_image(filename) {
    Ok(buffer) => Some(Image::new(width as u32, height as u32, buffer)),
    Err(_) => None,
  }
}
