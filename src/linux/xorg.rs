use crate::{DisplayInfo, Image};
use anyhow::{anyhow, Result};
use xcb::x::{Drawable, GetImage, ImageFormat, ImageOrder};

fn get_pixel8_rgba(
    bytes: &Vec<u8>,
    x: u32,
    y: u32,
    bytes_per_row: usize,
    bit_order: ImageOrder,
) -> (u8, u8, u8, u8) {
    let index = y as usize * bytes_per_row + x as usize;

    let pixel = if bit_order == ImageOrder::LsbFirst {
        bytes[index]
    } else {
        bytes[index] & 7 << 4 | bytes[index] >> 4
    };

    let r = (pixel >> 6) as f32 / 3.0 * 255.0;
    let g = ((pixel >> 2) & 7) as f32 / 7.0 * 255.0;
    let b = (pixel & 3) as f32 / 3.0 * 255.0;

    (r as u8, g as u8, b as u8, 255)
}

fn get_pixel16_rgba(
    bytes: &Vec<u8>,
    x: u32,
    y: u32,
    bytes_per_row: usize,
    bit_order: ImageOrder,
) -> (u8, u8, u8, u8) {
    let index = y as usize * bytes_per_row + x as usize;

    let pixel = if bit_order == ImageOrder::LsbFirst {
        bytes[index << 1] as u16 | (bytes[(index << 1) + 1] as u16) << 8
    } else {
        (bytes[index << 1] as u16) << 8 | bytes[(index << 1) + 1] as u16
    };

    let r = (pixel >> 11) as f32 / 31.0 * 255.0;
    let g = ((pixel >> 5) & 63) as f32 / 63.0 * 255.0;
    let b = (pixel & 31) as f32 / 31.0 * 255.0;

    (r as u8, g as u8, b as u8, 255)
}

fn get_pixel24_rgba(
    bytes: &Vec<u8>,
    x: u32,
    y: u32,
    bytes_per_row: usize,
    bit_order: ImageOrder,
) -> (u8, u8, u8, u8) {
    let index = y as usize * bytes_per_row + x as usize;

    if bit_order == ImageOrder::LsbFirst {
        (bytes[index + 2], bytes[index + 1], bytes[index], 255)
    } else {
        (bytes[index], bytes[index + 1], bytes[index + 2], 255)
    }
}

fn capture(x: i32, y: i32, width: u32, height: u32) -> Result<Image> {
    let (conn, index) = xcb::Connection::connect(None)?;

    let setup = conn.get_setup();
    let screen = setup
        .roots()
        .nth(index as usize)
        .ok_or_else(|| anyhow!("Not found screen"))?;

    let get_image_cookie = conn.send_request(&GetImage {
        format: ImageFormat::ZPixmap,
        drawable: Drawable::Window(screen.root()),
        x: x as i16,
        y: y as i16,
        width: width as u16,
        height: height as u16,
        plane_mask: u32::MAX,
    });

    let get_image_reply = conn.wait_for_reply(get_image_cookie)?;
    let bytes = Vec::from(get_image_reply.data());
    let depth = get_image_reply.depth();

    if depth == 32 {
        let image = Image::from_bgra(bytes, width, height, (width as usize) * 4);
        Ok(image)
    } else {
        let size = (width * height * 4) as usize;
        let mut rgba = vec![0u8; size];
        let bit_order = setup.bitmap_format_bit_order();
        for y in 0..height {
            for x in 0..width {
                let index = ((y * width + x) * 4) as usize;

                let (r, g, b, a) = match depth {
                    8 => get_pixel8_rgba(&bytes, x, y, width as usize / 2, bit_order),
                    16 => get_pixel16_rgba(&bytes, x, y, width as usize, bit_order),
                    24 => get_pixel24_rgba(&bytes, x, y, width as usize * 3, bit_order),
                    _ => return Err(anyhow!("Unsupported {} depth", depth)),
                };

                rgba[index] = r;
                rgba[index + 1] = g;
                rgba[index + 2] = b;
                rgba[index + 3] = a;
            }
        }

        let image = Image::new(width, height, rgba);
        Ok(image)
    }
}

pub fn xorg_capture_screen(display_info: &DisplayInfo) -> Result<Image> {
    let x = ((display_info.x as f32) * display_info.scale_factor) as i32;
    let y = ((display_info.y as f32) * display_info.scale_factor) as i32;
    let width = ((display_info.width as f32) * display_info.scale_factor) as u32;
    let height = ((display_info.height as f32) * display_info.scale_factor) as u32;

    capture(x, y, width, height)
}

pub fn xorg_capture_screen_area(
    display_info: &DisplayInfo,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<Image> {
    let area_x = (((x + display_info.x) as f32) * display_info.scale_factor) as i32;
    let area_y = (((y + display_info.y) as f32) * display_info.scale_factor) as i32;
    let area_width = ((width as f32) * display_info.scale_factor) as u32;
    let area_height = ((height as f32) * display_info.scale_factor) as u32;

    capture(area_x, area_y, area_width, area_height)
}
