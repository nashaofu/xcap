mod wayland;
mod xorg;

use crate::Image;
use crate::ScreenCapturer;

use std::env::var_os;
use wayland::wayland_capture_display;
use xorg::xorg_capture_display;

fn wayland_dectected() -> bool {
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

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  if wayland_dectected() {
    wayland_capture_display(&screen_capturer)
  } else {
    xorg_capture_display(&screen_capturer)
  }
}
