mod wayland;
mod xorg;

use crate::Image;
use crate::Screen;

use std::env::var_os;
use wayland::wayland_capture_screen;
use xorg::xorg_capture_screen;

fn wayland_detect() -> bool {
  let xdg_session_type = var_os("XDG_SESSION_TYPE")
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();

  let wayland_display = var_os("WAYLAND_DISPLAY")
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();

  return xdg_session_type.eq("wayland") || wayland_display.to_lowercase().contains("wayland");
}

pub fn capture_screen(screen: &Screen) -> Option<Image> {
  if wayland_detect() {
    wayland_capture_screen(&screen)
  } else {
    xorg_capture_screen(&screen)
  }
}
