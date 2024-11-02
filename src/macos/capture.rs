use core_graphics::{
    display::{kCGWindowImageDefault, CGWindowID, CGWindowListOption},
    geometry::CGRect,
    window::create_image,
};
use image::RgbaImage;

use crate::error::{XCapError, XCapResult};

pub fn capture(
    cg_rect: CGRect,
    list_option: CGWindowListOption,
    window_id: CGWindowID,
) -> XCapResult<RgbaImage> {
    let cg_image = create_image(cg_rect, list_option, window_id, kCGWindowImageDefault)
        .ok_or_else(|| XCapError::new(format!("Capture failed {} {:?}", window_id, cg_rect)))?;

    let width = cg_image.width();
    let height = cg_image.height();
    let bytes = Vec::from(cg_image.data().bytes());

    // Some platforms e.g. MacOS can have extra bytes at the end of each row.
    // See
    // https://github.com/nashaofu/xcap/issues/29
    // https://github.com/nashaofu/xcap/issues/38
    let mut buffer = Vec::with_capacity(width * height * 4);
    for row in bytes.chunks_exact(cg_image.bytes_per_row()) {
        buffer.extend_from_slice(&row[..width * 4]);
    }

    for bgra in buffer.chunks_exact_mut(4) {
        bgra.swap(0, 2);
    }

    RgbaImage::from_raw(width as u32, height as u32, buffer)
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
}

pub fn capture_bytes(
    cg_rect: CGRect,
    list_option: CGWindowListOption,
    window_id: CGWindowID,
) -> XCapResult<Vec<u8>> {
    let cg_image = create_image(cg_rect, list_option, window_id, kCGWindowImageDefault)
        .ok_or_else(|| XCapError::new(format!("Capture failed {} {:?}", window_id, cg_rect)))?;

    let bytes = Vec::from(cg_image.data().bytes());
    Ok(bytes)
}
