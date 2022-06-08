mod wayland;
mod xorg;

use crate::Image;
use crate::ScreenCapturer;

use std::panic;
use wayland::wayland_capture_display;
use xorg::xorg_capture_display;

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  match panic::catch_unwind(|| xorg_capture_display(&screen_capturer)) {
    Ok(image) => image,
    Err(_) => wayland_capture_display(&screen_capturer),
  }
}
