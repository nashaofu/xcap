use crate::{Image, ScreenCapturer};
use core_graphics::display::CGDisplay;

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  let cg_display = CGDisplay::new(screen_capturer.display_info.id);
  let cg_image = cg_display.image()?;

  Some(Image {
    width: cg_image.width() as u32,
    height: cg_image.height() as u32,
    bytes: Vec::from(cg_image.data().bytes()),
  })
}
