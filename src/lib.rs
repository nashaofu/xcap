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
  pub rotation: f32,
  pub scale_factor: f32,
  pub is_primary: bool,
}

impl Screen {
  pub fn new(display_info: DisplayInfo) -> Self {
    Screen {
      id: display_info.id,
      x: display_info.x,
      y: display_info.y,
      width: display_info.width,
      height: display_info.height,
      rotation: display_info.rotation,
      scale_factor: display_info.scale_factor,
      is_primary: display_info.is_primary,
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

  /**
   * 截取指定区域
   * 区域x,y为相对于当前屏幕的x,y坐标
   */
  pub fn capture_area(&self, x: i32, y: i32, width: u32, height: u32) -> Option<Image> {
    let screen_x2 = self.x + self.width as i32;
    let screen_y2 = self.y + self.height as i32;

    let mut x1 = x + self.x;
    let mut y1 = y + self.y;
    let mut x2 = x1 + width as i32;
    let mut y2 = y1 + height as i32;

    // x y 必须在屏幕范围内
    if x1 < self.x {
      x1 = self.x;
    } else if x1 > screen_x2 {
      x1 = screen_x2
    }

    if y1 < self.y {
      y1 = self.y;
    } else if y1 > screen_y2 {
      y1 = screen_y2;
    }

    if x2 > screen_x2 {
      x2 = screen_x2;
    }

    if y2 > screen_y2 {
      y2 = screen_y2;
    }

    if x1 >= x2 || y1 >= y2 {
      return None;
    }

    capture_screen_area(
      &self,
      x1 - self.x,
      y1 - self.y,
      (x2 - x1) as u32,
      (y2 - y1) as u32,
    )
  }
}
