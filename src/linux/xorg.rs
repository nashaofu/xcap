use crate::{Image, Screenshots};
use std::{ptr, slice};
use x11::xlib::{XAllPlanes, XCloseDisplay, XDefaultRootWindow, XGetImage, XOpenDisplay, ZPixmap};

pub fn xorg_capture_display(screenshots: &Screenshots) -> Option<Image> {
  unsafe {
    let display_ptr = XOpenDisplay(ptr::null_mut());

    if display_ptr.is_null() {
      return None;
    }

    let window_id = XDefaultRootWindow(display_ptr);
    let display_info = screenshots.display_info;
    let x = ((display_info.x as f32) * display_info.scale) as i32;
    let y = ((display_info.y as f32) * display_info.scale) as i32;
    let width = ((display_info.width as f32) * display_info.scale) as u32;
    let height = ((display_info.height as f32) * display_info.scale) as u32;

    let ximage = XGetImage(
      display_ptr,
      window_id,
      x,
      y,
      width,
      height,
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

    match Image::from_bgr(width as u32, height as u32, bytes) {
      Ok(image) => Some(image),
      Err(_) => None,
    }
  }
}
