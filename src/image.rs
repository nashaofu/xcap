use png::{BitDepth, ColorType, Encoder, EncodingError};

pub struct Image {
  pub width: u32,
  pub height: u32,
  pub bytes: Vec<u8>,
}

impl Image {
  pub fn png(&self) -> Result<Vec<u8>, EncodingError> {
    let mut buffer = Vec::new();
    let width = self.width as u32;
    let height = self.height as u32;
    let mut bytes = self.bytes.clone();

    // BGR 转换为 RGB
    for i in (0..bytes.len()).step_by(4) {
      let b = bytes[i];
      let r = bytes[i + 2];

      bytes[i] = r;
      bytes[i + 2] = b;
      bytes[i + 3] = 255;
    }

    let mut encoder = Encoder::new(&mut buffer, width, height);

    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&bytes)?;
    writer.finish()?;

    Ok(buffer)
  }
}
