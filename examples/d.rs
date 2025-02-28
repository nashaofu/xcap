use std::fs;

use image::RgbaImage;

fn main() {
    let frame = fs::read("./frame_0.rgb").unwrap();
    let img = RgbaImage::from_raw(3584, 2240, frame).unwrap();
    img.save("a.png").unwrap()
}
