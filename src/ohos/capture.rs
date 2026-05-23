//! OHOS single-frame capture via PixelMap (API 14+).

use image::RgbaImage;

use crate::error::{XCapError, XCapResult};

use super::ffi;

fn ensure_image_info_query(result: i32, query_name: &str) -> XCapResult<()> {
    if result == ffi::IMAGE_SUCCESS {
        return Ok(());
    }

    Err(XCapError::new(format!("{query_name} failed: {result}",)))
}

/// Capture one RGBA frame from `display_id`.
///
/// `width` and `height` are kept for API compatibility with other platforms.
pub fn capture_screen(display_id: u64, _width: u32, _height: u32) -> XCapResult<RgbaImage> {
    let mut pixelmap: *mut ffi::OH_PixelmapNative = std::ptr::null_mut();
    let rc = unsafe {
        ffi::OH_NativeDisplayManager_CaptureScreenPixelmap(display_id as u32, &mut pixelmap)
    };

    if rc != 0 {
        return Err(XCapError::new(format!(
            "OH_NativeDisplayManager_CaptureScreenPixelmap failed: rc={}",
            rc
        )));
    }
    if pixelmap.is_null() {
        return Err(XCapError::new(
            "CaptureScreenPixelmap returned null pixelmap",
        ));
    }

    struct PixelmapGuard(*mut ffi::OH_PixelmapNative);
    impl Drop for PixelmapGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ffi::OH_PixelmapNative_Release(self.0) };
            }
        }
    }
    let _guard = PixelmapGuard(pixelmap);

    let mut info_ptr: *mut ffi::OH_Pixelmap_ImageInfo = std::ptr::null_mut();
    let info_create_rc = unsafe { ffi::OH_PixelmapImageInfo_Create(&mut info_ptr) };
    if info_create_rc != ffi::IMAGE_SUCCESS || info_ptr.is_null() {
        return Err(XCapError::new(format!(
            "OH_PixelmapImageInfo_Create failed: {}",
            info_create_rc
        )));
    }

    struct ImageInfoGuard(*mut ffi::OH_Pixelmap_ImageInfo);
    impl Drop for ImageInfoGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ffi::OH_PixelmapImageInfo_Release(self.0) };
            }
        }
    }
    let _info_guard = ImageInfoGuard(info_ptr);

    let info_rc = unsafe { ffi::OH_PixelmapNative_GetImageInfo(pixelmap, info_ptr) };
    if info_rc != ffi::IMAGE_SUCCESS {
        return Err(XCapError::new(format!(
            "OH_PixelmapNative_GetImageInfo failed: {}",
            info_rc
        )));
    }

    let (mut width, mut height, mut row_stride, mut pixel_format) = (0u32, 0u32, 0u32, 0i32);
    unsafe {
        ensure_image_info_query(
            ffi::OH_PixelmapImageInfo_GetWidth(info_ptr, &mut width),
            "OH_PixelmapImageInfo_GetWidth",
        )?;
        ensure_image_info_query(
            ffi::OH_PixelmapImageInfo_GetHeight(info_ptr, &mut height),
            "OH_PixelmapImageInfo_GetHeight",
        )?;
        ensure_image_info_query(
            ffi::OH_PixelmapImageInfo_GetRowStride(info_ptr, &mut row_stride),
            "OH_PixelmapImageInfo_GetRowStride",
        )?;
        ensure_image_info_query(
            ffi::OH_PixelmapImageInfo_GetPixelFormat(info_ptr, &mut pixel_format),
            "OH_PixelmapImageInfo_GetPixelFormat",
        )?;
    }

    if width == 0 || height == 0 || row_stride == 0 {
        return Err(XCapError::new(format!(
            "invalid pixelmap info: {}x{} stride={}",
            width, height, row_stride
        )));
    }

    let stride = row_stride as usize;
    let bytes_per_row = width as usize * 4;
    if stride < bytes_per_row {
        return Err(XCapError::new(format!(
            "invalid row stride: {} < {}",
            stride, bytes_per_row
        )));
    }

    let total_bytes = stride * height as usize;
    let mut buf = vec![0u8; total_bytes];
    let mut buf_size = total_bytes;
    let read_rc =
        unsafe { ffi::OH_PixelmapNative_ReadPixels(pixelmap, buf.as_mut_ptr(), &mut buf_size) };
    if read_rc != ffi::IMAGE_SUCCESS {
        return Err(XCapError::new(format!(
            "OH_PixelmapNative_ReadPixels failed: {}",
            read_rc
        )));
    }

    let is_bgra = pixel_format == ffi::PIXEL_FORMAT_BGRA_8888;
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height as usize {
        let src_start = row * stride;
        let src_row = &buf[src_start..src_start + bytes_per_row];
        if is_bgra {
            for chunk in src_row.chunks_exact(4) {
                pixels.push(chunk[2]);
                pixels.push(chunk[1]);
                pixels.push(chunk[0]);
                pixels.push(chunk[3]);
            }
        } else {
            pixels.extend_from_slice(src_row);
        }
    }

    RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed for pixelmap capture"))
}
