use crate::{Image, ScreenCapturer};
use std::{ptr, slice};
use x11::xlib::{XAllPlanes, XCloseDisplay, XDefaultRootWindow, XGetImage, XOpenDisplay, ZPixmap};

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  unsafe {
    let display_ptr = XOpenDisplay(ptr::null_mut());

    if display_ptr.is_null() {
      return None;
    }

    let window_id = XDefaultRootWindow(display_ptr);
    let display_info = screen_capturer.display_info;

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

    let data = (*ximage).data;
    let width = (*ximage).width;
    let height = (*ximage).height;
    let bytes = Vec::from(slice::from_raw_parts(
      data as *mut u8,
      (width * height * 4) as usize,
    ));

    Some(Image {
      width: width as u32,
      height: height as u32,
      bytes,
    })
  }
}
