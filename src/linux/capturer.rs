use super::Image;
use crate::Capturer;
use std::ptr;
use x11::xlib::{XAllPlanes, XCloseDisplay, XDefaultRootWindow, XGetImage, XOpenDisplay, ZPixmap};

impl Capturer {
  pub fn capture_screen(&self) -> Option<Image> {
    unsafe {
      let display_ptr = XOpenDisplay(ptr::null_mut());

      if display_ptr.is_null() {
        return None;
      }

      let window_id = XDefaultRootWindow(display_ptr);
      let display_info = self.display_info;

      let ximage = XGetImage(
        display_ptr,
        window_id,
        display_info.x,
        display_info.y,
        display_info.width,
        display_info.height,
        XAllPlanes(),
        ZPixmap,
      );

      if ximage.is_null() {
        return None;
      }

      XCloseDisplay(display_ptr);

      Some(Image::new(ximage))
    }
  }
}
