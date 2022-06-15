use crate::{Image, Screen};
use core_graphics::display::CGDisplay;

pub fn capture_screen(screen: &Screen) -> Option<Image> {
  let cg_display = CGDisplay::new(screen.id);
  let cg_image = cg_display.image()?;

  match Image::from_bgr(
    cg_image.width() as u32,
    cg_image.height() as u32,
    Vec::from(cg_image.data().bytes()),
  ) {
    Ok(image) => Some(image),
    Err(_) => None,
  }
}
