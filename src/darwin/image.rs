use core_graphics::image::CGImage;
use png::{BitDepth, ColorType, Encoder, ScaledFloat, SourceChromaticities};
use std::{any::Any, fmt};

pub struct Image {
  cg_image: CGImage,
}

impl Image {
  pub fn new(cg_image: CGImage) -> Self {
    Image { cg_image }
  }

  pub fn width(&self) -> usize {
    self.cg_image.width()
  }

  pub fn height(&self) -> usize {
    self.cg_image.height()
  }

  pub fn bytes(&self) -> Vec<u8> {
    let data = self.cg_image.data();
    let mut bytes = Vec::from(data.bytes());

    // BGR 转换为 RGB
    for i in (0..bytes.len()).step_by(4) {
      let b = bytes[i];
      let r = bytes[i + 2];

      bytes[i] = r;
      bytes[i + 2] = b;
    }

    return bytes;
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

impl fmt::Debug for Image {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Image")
      .field("cg_image", &self.cg_image.type_id())
      .finish()
  }
}
