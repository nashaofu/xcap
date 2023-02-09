use png::{BitDepth, ColorType, Encoder, EncodingError};

pub struct Image {
  width: u32,
  height: u32,
  buffer: Vec<u8>,
}

impl Image {
  pub fn new(width: u32, height: u32, buffer: Vec<u8>) -> Self {
    Image {
      width,
      height,
      buffer,
    }
  }

  pub fn from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Result<Self, EncodingError> {
    let mut buffer = Vec::new();
    let size = (width * height * 4) as usize;

    // https://github.com/nashaofu/screenshots-rs/issues/38
    // bgra 长度保证为 width * height * 4
    let mut bytes = Vec::from(&bgra[..size]);

    // BGRA 转换为 RGBA
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

    Ok(Image::new(width, height, buffer))
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  pub fn buffer(&self) -> &Vec<u8> {
    &self.buffer
  }
}

impl Into<Vec<u8>> for Image {
  fn into(self) -> Vec<u8> {
    self.buffer
  }
}
