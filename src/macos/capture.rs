use core_graphics::{
    display::{kCGWindowImageDefault, CGWindowID, CGWindowListOption},
    geometry::CGRect,
    window::create_image,
};
use image::RgbaImage;

use crate::{
    error::{XCapError, XCapResult},
    utils::image::{bgra_to_rgba_image, remove_extra_data},
};

pub fn capture(
    cg_rect: CGRect,
    list_option: CGWindowListOption,
    window_id: CGWindowID,
) -> XCapResult<RgbaImage> {
    let cg_image = create_image(cg_rect, list_option, window_id, kCGWindowImageDefault)
        .ok_or_else(|| XCapError::new(format!("Capture failed {} {:?}", window_id, cg_rect)))?;

    let width = cg_image.width();
    let height = cg_image.height();
    let clean_buf = remove_extra_data(
        width,
        height,
        cg_image.bytes_per_row(),
        Vec::from(cg_image.data().bytes()),
    );

    bgra_to_rgba_image(width as u32, height as u32, clean_buf)
}
