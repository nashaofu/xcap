use png::{BitDepth, ColorType, Encoder, ScaledFloat, SourceChromaticities};
use std::slice;
use x11::xlib::{XDestroyImage, XImage};

#[derive(Debug)]
pub struct Image {
  ximage: *mut XImage,
}

impl Image {
  pub fn new(ximage: *mut XImage) -> Self {
    Image { ximage }
  }

  pub fn width(&self) -> usize {
    unsafe { (*(self.ximage)).width as usize }
  }

  pub fn height(&self) -> usize {
    unsafe { (*(self.ximage)).height as usize }
  }

  pub fn bytes(&self) -> Vec<u8> {
    unsafe {
      let data = (*(self.ximage)).data;
      let mut bytes = Vec::from(slice::from_raw_parts(
        data as *mut u8,
        self.width() * self.height() * 4,
      ));

      // BGR 转换为 RGB
      for i in (0..bytes.len()).step_by(4) {
        let b = bytes[i];
        let r = bytes[i + 2];

        bytes[i] = r;
        bytes[i + 2] = b;
      }

      return bytes;
    }
  }

  pub fn png(&self) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
      let width = self.width() as u32;
      let height = self.height() as u32;
      let bytes = self.bytes();

      let mut encoder = Encoder::new(&mut buffer, width, height);

      encoder.set_color(ColorType::Rgba);
      encoder.set_depth(BitDepth::Eight);
      encoder.set_trns(vec![0xFFu8, 0xFFu8, 0xFFu8, 0xFFu8]);

      // 1.0 / 2.2, scaled by 100000
      encoder.set_source_gamma(ScaledFloat::from_scaled(45455));
      // 1.0 / 2.2, unscaled, but rounded
      encoder.set_source_gamma(ScaledFloat::new(1.0 / 2.2));

      let source_chromaticities = SourceChromaticities::new(
        // Using unscaled instantiation here
        (0.31270, 0.32900),
        (0.64000, 0.33000),
        (0.30000, 0.60000),
        (0.15000, 0.06000),
      );

      encoder.set_source_chromaticities(source_chromaticities);

      let mut writer = encoder.write_header().unwrap();

      writer.write_image_data(&bytes).unwrap();
    }

    return buffer;
  }
}

impl Drop for Image {
  fn drop(&mut self) {
    unsafe {
      XDestroyImage(self.ximage);
    }
  }
}
