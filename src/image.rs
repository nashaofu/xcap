use png::{BitDepth, ColorType, Encoder, EncodingError};

pub struct Image {
  width: u32,
  height: u32,
  buffer: Vec<u8>,
  pixels: Option<Vec<Pixel>>,
}
pub struct Pixel {
  pub x: u32,
  pub y: u32,
  pub r: u8,
  pub g: u8,
  pub b: u8,
}

impl Image {
  pub fn new(width: u32, height: u32, buffer: Vec<u8>, pixels: Vec<Pixel>) -> Self {
    Image {
      width,
      height,
      buffer,
      pixels: Some(pixels),
    }
  }

  pub fn from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Result<Self, EncodingError> {
    let mut buffer = Vec::new();
    let mut bytes = bgra;

    // BGRA convert to RGBA
    let mut pixels = Vec::new();
    for i in (0..bytes.len()).step_by(4) {
      // Get RGB values
      let b = bytes[i];
      let g = bytes[i + 1];
      let r = bytes[i + 2];

      // Get pixel position
      let x = (i / 4) as u32 % width;
      let y = (i / 4) as u32 / width;

      // Push pixel to vector
      let pixel = Pixel { x, y, r, g, b };
      pixels.push(pixel);

      // Convert to RGBA
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

    // return
    Ok(Image::new(width, height, buffer, pixels))
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

  pub fn pixels(&self) -> &Vec<Pixel> {
    self.pixels.as_ref().unwrap()
  }

  // get pixel at x, y and return Pixel struct with x, y, r, g, b values
  pub fn get_pixel(&self, x: u32, y: u32) -> &Pixel {
    self.pixels.as_ref().unwrap().iter().find(|pixel| pixel.x == x && pixel.y == y).unwrap()
  }
}
