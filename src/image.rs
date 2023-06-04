use png::{BitDepth, ColorType, Encoder, EncodingError};

pub struct Image {
  width: u32,
  height: u32,
  rgba: Vec<u8>,
}

impl Image {
  pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
    Image {
      width,
      height,
      rgba,
    }
  }

  pub fn from_bgra(bgra: Vec<u8>, width: u32, height: u32, bytes_per_row: usize) -> Self {
    let size = (width * height * 4) as usize;
    let mut rgba = vec![0u8; size];

    let u_width = width as usize;
    let u_height = height as usize;

    // 数据对齐，有时传入 bgra 每一行像素点多余宽度值
    // 例如在 mac 上，截图尺寸为10*10时，返回的数据长度大于400
    // https://github.com/nashaofu/screenshots-rs/issues/29
    // https://github.com/nashaofu/screenshots-rs/issues/38
    // BGRA 转换为 RGBA
    for r in 0..u_height {
      for c in 0..u_width {
        let index = (r * u_width + c) * 4;
        let i = r * bytes_per_row + c * 4;
        let b = bgra[i];
        let r = bgra[i + 2];

        rgba[index] = r;
        rgba[index + 1] = bgra[i + 1];
        rgba[index + 2] = b;
        rgba[index + 3] = 255;
      }
    }

    Image::new(width, height, rgba)
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  pub fn rgba(&self) -> &Vec<u8> {
    &self.rgba
  }

  pub fn to_png(&self) -> Result<Vec<u8>, EncodingError> {
    let mut buffer = Vec::new();
    let mut encoder = Encoder::new(&mut buffer, self.width, self.height);

    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&self.rgba)?;
    writer.finish()?;

    Ok(buffer)
  }
}

impl Into<Vec<u8>> for Image {
  fn into(self) -> Vec<u8> {
    self.rgba
  }
}
