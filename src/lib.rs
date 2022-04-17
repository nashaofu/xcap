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
pub use win32::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[derive(Debug, Clone, Copy)]
pub struct ScreenCapturer {
  pub display_info: DisplayInfo,
}

impl ScreenCapturer {
  pub fn new(display_info: DisplayInfo) -> Self {
    ScreenCapturer { display_info }
  }

  pub fn all() -> Vec<ScreenCapturer> {
    DisplayInfo::all()
      .iter()
      .map(move |display_info| ScreenCapturer::new(*display_info))
      .collect()
  }

  pub fn from_point(x: i32, y: i32) -> Option<ScreenCapturer> {
    match DisplayInfo::from_point(x, y) {
      Some(display_info) => Some(ScreenCapturer::new(display_info)),
      None => None,
    }
  }
  pub fn capture(&self) -> Option<Image> {
    capture_display(&self)
  }
}
