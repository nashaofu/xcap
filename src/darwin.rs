use crate::{DisplayInfo, Image};
use core_graphics::display::{CGDisplay, CGPoint, CGRect, CGSize};

pub fn capture_screen(display_info: &DisplayInfo) -> Option<Image> {
  let cg_display = CGDisplay::new(display_info.id);
  let cg_image = cg_display.image()?;

  Image::from_bgra(
    cg_image.width() as u32,
    cg_image.height() as u32,
    Vec::from(cg_image.data().bytes()),
  )
  .ok()
}

pub fn capture_screen_area(
  display_info: &DisplayInfo,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  let cg_display = CGDisplay::new(display_info.id);
  let full_cg_image = cg_display.image()?;

  let w = width as f32 * display_info.scale_factor;
  let h = height as f32 * display_info.scale_factor;

  let cg_rect = CGRect::new(
    &CGPoint::new(x as f64, y as f64),
    &CGSize::new(w as f64, h as f64),
  );

  let cg_image = full_cg_image.cropped(cg_rect)?;

  Image::from_bgra(
    cg_image.width() as u32,
    cg_image.height() as u32,
    Vec::from(cg_image.data().bytes()),
  )
  .ok()
}
