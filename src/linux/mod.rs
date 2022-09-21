mod wayland;
mod wayland_screenshot;
mod xorg;

use crate::{DisplayInfo, Image};

use std::env::var_os;
use wayland::{wayland_capture_screen, wayland_capture_screen_area};
use xorg::{xorg_capture_screen, xorg_capture_screen_area};

fn wayland_detect() -> bool {
  let xdg_session_type = var_os("XDG_SESSION_TYPE")
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();

  let wayland_display = var_os("WAYLAND_DISPLAY")
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();

  xdg_session_type.eq("wayland") || wayland_display.to_lowercase().contains("wayland")
}

pub fn capture_screen(display_info: &DisplayInfo) -> Option<Image> {
  if wayland_detect() {
    wayland_capture_screen(display_info)
  } else {
    xorg_capture_screen(display_info)
  }
}

pub fn capture_screen_area(
  display_info: &DisplayInfo,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  if wayland_detect() {
    wayland_capture_screen_area(display_info, x, y, width, height)
  } else {
    xorg_capture_screen_area(display_info, x, y, width, height)
  }
}
