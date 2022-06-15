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
pub struct Screen {
  pub id: u32,
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
  pub scale: f32,
  pub rotation: f32,
}

impl Screen {
  pub fn new(display_info: DisplayInfo) -> Self {
    Screen {
      id: display_info.id,
      x: display_info.x,
      y: display_info.y,
      width: display_info.width,
      height: display_info.height,
      scale: display_info.scale,
      rotation: display_info.rotation,
    }
  }

  pub fn all() -> Vec<Screen> {
    DisplayInfo::all()
      .iter()
      .map(move |display_info| Screen::new(*display_info))
      .collect()
  }

  pub fn from_point(x: i32, y: i32) -> Option<Screen> {
    let display_info = DisplayInfo::from_point(x, y)?;
    Some(Screen::new(display_info))
  }

  pub fn capture(&self) -> Option<Image> {
    capture_screen(&self)
  }
}
