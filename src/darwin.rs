use crate::{Image, Screen};
use core_graphics::display::{CGDisplay, CGPoint, CGRect, CGSize};

pub fn capture_screen(screen: &Screen) -> Option<Image> {
  let cg_display = CGDisplay::new(screen.id);
  let cg_image = cg_display.image()?;

  match Image::from_bgra(
    cg_image.width() as u32,
    cg_image.height() as u32,
    Vec::from(cg_image.data().bytes()),
  ) {
    Ok(image) => Some(image),
    Err(_) => None,
  }
}

pub fn capture_screen_area(
  screen: &Screen,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
) -> Option<Image> {
  let cg_display = CGDisplay::new(screen.id);
  let full_cg_image = cg_display.image()?;

  let w = width as f32 * screen.scale_factor;
  let h = height as f32 * screen.scale_factor;

  let cg_rect = CGRect::new(
    &CGPoint::new(x as f64, y as f64),
    &CGSize::new(w as f64, h as f64),
  );

  let cg_image = full_cg_image.cropped(cg_rect)?;

  match Image::from_bgra(
    cg_image.width() as u32,
    cg_image.height() as u32,
    Vec::from(cg_image.data().bytes()),
  ) {
    Ok(image) => Some(image),
    Err(_) => None,
  }
}
