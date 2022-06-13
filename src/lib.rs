use display_info::DisplayInfo;

mod image;
pub use image::Image;

#[cfg(target_os = "macos")]
mod darwin;
#[cfg(target_os = "macos")]
use darwin::*;

#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "windows")]
use win32::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::*;

#[derive(Debug, Clone, Copy)]
pub struct Screenshots {
  pub display_info: DisplayInfo,
}

impl Screenshots {
  pub fn new(display_info: DisplayInfo) -> Self {
    Screenshots { display_info }
  }

  pub fn all() -> Vec<Screenshots> {
    DisplayInfo::all()
      .iter()
      .map(move |display_info| Screenshots::new(*display_info))
      .collect()
  }

  pub fn from_point(x: i32, y: i32) -> Option<Screenshots> {
    let display_info = DisplayInfo::from_point(x, y)?;
    Some(Screenshots::new(display_info))
  }

  pub fn capture(&self) -> Option<Image> {
    capture_display(&self)
  }
}
