mod wayland;
mod xorg;

use crate::Image;
use crate::ScreenCapturer;

use std::env::var_os;
use wayland::wayland_capture_display;
use xorg::xorg_capture_display;

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  let xdg_session_type = var_os("XDG_SESSION_TYPE")
    .map(|str| str.to_string_lossy().to_string())
    .unwrap_or(String::from("x11"));

  println!("XDG_SESSION_TYPE: {}", xdg_session_type);

  // TODO: 这里判断需改进
  if xdg_session_type.eq("x11") {
    xorg_capture_display(&screen_capturer)
  } else {
    wayland_capture_display(&screen_capturer)
  }
}
