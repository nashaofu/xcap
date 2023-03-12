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

  pub fn from_bgra(
    bgra: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_row: usize,
  ) -> Result<Self, EncodingError> {
    let mut buffer = Vec::new();
    let size = (width * height * 4) as usize;
    let mut bytes = vec![0u8; size];

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

        bytes[index] = r;
        bytes[index + 1] = bgra[i + 1];
        bytes[index + 2] = b;
        bytes[index + 3] = 255;
      }
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
