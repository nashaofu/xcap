use image::RgbaImage;
use objc2_core_foundation::CGRect;
use objc2_core_graphics::{
    CGDataProvider, CGImage, CGWindowID, CGWindowImageOption, CGWindowListCreateImage,
    CGWindowListOption,
};

use crate::error::{XCapError, XCapResult};

pub fn capture(
    cg_rect: CGRect,
    list_option: CGWindowListOption,
    window_id: CGWindowID,
) -> XCapResult<RgbaImage> {
    unsafe {
        let cg_image = CGWindowListCreateImage(
            cg_rect,
            list_option,
            window_id,
            CGWindowImageOption::Default,
        );

        let width = CGImage::width(cg_image.as_deref());
        let height = CGImage::height(cg_image.as_deref());
        let data_provider = CGImage::data_provider(cg_image.as_deref());

        let data = CGDataProvider::data(data_provider.as_deref())
            .ok_or_else(|| XCapError::new("Failed to copy data"))?
            .to_vec();

        let bytes_per_row = CGImage::bytes_per_row(cg_image.as_deref());

        // Some platforms e.g. MacOS can have extra bytes at the end of each row.
        // See
        // https://github.com/nashaofu/xcap/issues/29
        // https://github.com/nashaofu/xcap/issues/38
        let mut buffer = Vec::with_capacity(width * height * 4);
        for row in data.chunks_exact(bytes_per_row) {
            buffer.extend_from_slice(&row[..width * 4]);
        }

        for bgra in buffer.chunks_exact_mut(4) {
            bgra.swap(0, 2);
        }

        RgbaImage::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
    }
}
