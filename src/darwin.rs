use crate::{Image, Screenshots};
use core_graphics::display::CGDisplay;

pub fn capture_display(screenshots: &Screenshots) -> Option<Image> {
  let cg_display = CGDisplay::new(screenshots.display_info.id);
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
