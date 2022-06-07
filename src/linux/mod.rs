mod wayland;
mod xorg;

use crate::Image;
use crate::ScreenCapturer;

use wayland::wayland_capture_display;
use xorg::xorg_capture_display;

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  if cfg!(target_os = "macos") {
    println!("Think Different! {}", cfg!(target_os = "linux"));
    wayland_capture_display(&screen_capturer)
  } else {
    xorg_capture_display(&screen_capturer)
  }
}
