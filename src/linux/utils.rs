use image::{open, RgbaImage};

use crate::error::XCapResult;

pub(super) struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Rect {
    // 计算两个矩形的交集面积
    pub(super) fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    // 计算两个矩形的交集面积
    pub(super) fn overlap_area(&self, other_rect: Rect) -> i32 {
        let left = self.x.max(other_rect.x);
        let top = self.y.max(other_rect.y);
        let right = (self.x + self.width as i32).min(other_rect.x + other_rect.width as i32);
        let bottom = (self.y + self.height as i32).min(other_rect.y + other_rect.height as i32);

        // 与0比较，如果小于0则表示两个矩形无交集
        let width = (right - left).max(0);
        let height = (bottom - top).max(0);

        width * height
    }
}

pub(super) fn png_to_rgba_image(
    filename: &String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let mut dynamic_image = open(filename)?;
    dynamic_image = dynamic_image.crop(x as u32, y as u32, width as u32, height as u32);
    Ok(dynamic_image.to_rgba8())
}
