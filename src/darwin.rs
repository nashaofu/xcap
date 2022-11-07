use crate::{DisplayInfo, Image};
use core_graphics::display::CGDisplay;

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
  let cg_image = cg_display.image()?;

  let w = (width as f32 * display_info.scale_factor) as i32;
  let h = (height as f32 * display_info.scale_factor) as i32;

  let mut bgra = vec![0; (w * h * 4) as usize];
  let data = cg_image.data();
  let bytes = data.bytes();

  // 图片裁剪
  for r in y..(y + h) {
    for c in x..(x + w) {
      let index = (((r - y) * w + (c - x)) * 4) as usize;
      let i = ((r * cg_image.width() as i32 + c) * 4) as usize;
      bgra[index] = bytes[i];
      bgra[index + 1] = bytes[i + 1];
      bgra[index + 2] = bytes[i + 2];
      bgra[index + 3] = bytes[i + 3];
    }
  }

  Image::from_bgra(w as u32, h as u32, bgra).ok()
}
