use crate::{Image, Screen};
use std::{ptr, slice};
use x11::xlib::{XAllPlanes, XCloseDisplay, XDefaultRootWindow, XGetImage, XOpenDisplay, ZPixmap};

fn capture(x: i32, y: i32, width: u32, height: u32) -> Option<Image> {
  unsafe {
    let display_ptr = XOpenDisplay(ptr::null_mut());

    if display_ptr.is_null() {
      return None;
    }

    let window_id = XDefaultRootWindow(display_ptr);

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

    XCloseDisplay(display_ptr);

    if ximage.is_null() {
      return None;
    }

    let data = (*ximage).data;
    let width = (*ximage).width;
    let height = (*ximage).height;
    let bytes = Vec::from(slice::from_raw_parts(
      data as *mut u8,
      (width * height * 4) as usize,
    ));

    match Image::from_bgra(width as u32, height as u32, bytes) {
      Ok(image) => Some(image),
      Err(_) => None,
    }
  }
}

pub fn xorg_capture_screen(screen: &Screen) -> Option<Image> {
  let x = ((screen.x as f32) * screen.scale) as i32;
  let y = ((screen.y as f32) * screen.scale) as i32;
  let width = ((screen.width as f32) * screen.scale) as u32;
  let height = ((screen.height as f32) * screen.scale) as u32;

  capture(x, y, width, height)
}

pub fn xorg_capture_screen_area(
  screen: &Screen,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  let area_x = (((x + screen.x) as f32) * screen.scale) as i32;
  let area_y = (((y + screen.y) as f32) * screen.scale) as i32;
  let area_width = ((width as f32) * screen.scale) as u32;
  let area_height = ((height as f32) * screen.scale) as u32;

  capture(area_x, area_y, area_width, area_height)
}
