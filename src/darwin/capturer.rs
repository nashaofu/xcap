use super::Image;
use crate::Capturer;
use core_graphics::display::CGDisplay;

impl Capturer {
  pub fn capture_screen(&self) -> Option<Image> {
    let cg_display = CGDisplay::new(self.display_info.id);
    match cg_display.image() {
      Some(cg_image) => Some(Image::new(cg_image)),
      None => None,
    }
  }
}
