use std::{ffi::c_void, mem};

use image::{DynamicImage, RgbaImage};
use scopeguard::guard;
use windows::Win32::{
    Foundation::{GetLastError, HWND},
    Graphics::{
        Dwm::DwmIsCompositionEnabled,
        Gdi::{
            BITMAP, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap,
            CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetCurrentObject,
            GetDIBits, GetObjectW, GetWindowDC, HBITMAP, HDC, OBJ_BITMAP, ReleaseDC, SRCCOPY,
            SelectObject,
        },
    },
    Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
    UI::WindowsAndMessaging::GetDesktopWindow,
};

use crate::error::{XCapError, XCapResult};

use super::utils::{bgra_to_rgba_image, get_os_major_version, get_window_info};

fn to_rgba_image(
    hdc_mem: HDC,
    h_bitmap: HBITMAP,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let buffer_size = width * height * 4;
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biSizeImage: buffer_size as u32,
            biCompression: 0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut buffer = vec![0u8; buffer_size as usize];

    unsafe {
        // 读取数据到 buffer 中
        let is_failed = GetDIBits(
            hdc_mem,
            h_bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        ) == 0;

        if is_failed {
            return Err(XCapError::new("Get RGBA data failed"));
        }
    };

    bgra_to_rgba_image(width as u32, height as u32, buffer)
}

fn delete_bitmap_object(val: HBITMAP) {
    unsafe {
        let succeed = DeleteObject(val.into()).as_bool();

        if !succeed {
            log::error!("DeleteObject({:?}) failed: {:?}", val, GetLastError());
        }
    }
}

#[allow(unused)]
pub fn capture_monitor(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    unsafe {
        let hwnd = GetDesktopWindow();
        let scope_guard_hdc_desktop_window = guard(GetWindowDC(Some(hwnd)), |val| {
            if ReleaseDC(Some(hwnd), val) != 1 {
                log::error!("ReleaseDC({:?}) failed: {:?}", val, GetLastError());
            }
        });

        // 内存中的HDC，使用 DeleteDC 函数释放
        // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-createcompatibledc
        let scope_guard_mem = guard(
            CreateCompatibleDC(Some(*scope_guard_hdc_desktop_window)),
            |val| {
                if !DeleteDC(val).as_bool() {
                    log::error!("DeleteDC({:?}) failed: {:?}", val, GetLastError());
                }
            },
        );

        let scope_guard_h_bitmap = guard(
            CreateCompatibleBitmap(*scope_guard_hdc_desktop_window, width, height),
            delete_bitmap_object,
        );

        // 使用SelectObject函数将这个位图选择到DC中
        SelectObject(*scope_guard_mem, (*scope_guard_h_bitmap).into());

        // 拷贝原始图像到内存
        // 这里不需要缩放图片，所以直接使用BitBlt
        // 如需要缩放，则使用 StretchBlt
        BitBlt(
            *scope_guard_mem,
            0,
            0,
            width,
            height,
            Some(*scope_guard_hdc_desktop_window),
            x,
            y,
            SRCCOPY,
        )?;

        to_rgba_image(*scope_guard_mem, *scope_guard_h_bitmap, width, height)
    }
}

#[allow(unused)]
pub fn capture_window(hwnd: HWND, scale_factor: f32) -> XCapResult<RgbaImage> {
    let window_info = get_window_info(hwnd)?;
    unsafe {
        let rc_window = window_info.rcWindow;

        let mut width = rc_window.right - rc_window.left;
        let mut height = rc_window.bottom - rc_window.top;

        let scope_guard_hdc_window = guard(GetWindowDC(Some(hwnd)), |val| {
            if ReleaseDC(Some(hwnd), val) != 1 {
                log::error!("ReleaseDC({:?}) failed: {:?}", val, GetLastError());
            }
        });

        let hgdi_obj = GetCurrentObject(*scope_guard_hdc_window, OBJ_BITMAP);
        let mut bitmap = BITMAP::default();

        let mut horizontal_scale = 1.0;
        let mut vertical_scale = 1.0;

        if GetObjectW(
            hgdi_obj,
            mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut c_void),
        ) != 0
        {
            width = bitmap.bmWidth;
            height = bitmap.bmHeight;
        }

        width = (width as f32 * scale_factor).ceil() as i32;
        height = (height as f32 * scale_factor).ceil() as i32;

        // 内存中的HDC，使用 DeleteDC 函数释放
        // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-createcompatibledc
        let scope_guard_hdc_mem = guard(CreateCompatibleDC(Some(*scope_guard_hdc_window)), |val| {
            if !DeleteDC(val).as_bool() {
                log::error!("DeleteDC({:?}) failed: {:?}", val, GetLastError());
            }
        });
        let scope_guard_h_bitmap = guard(
            CreateCompatibleBitmap(*scope_guard_hdc_window, width, height),
            delete_bitmap_object,
        );

        let previous_object = SelectObject(*scope_guard_hdc_mem, (*scope_guard_h_bitmap).into());

        let mut is_success = false;

        // https://webrtc.googlesource.com/src.git/+/refs/heads/main/modules/desktop_capture/win/window_capturer_win_gdi.cc#301
        if get_os_major_version() >= 8 {
            is_success = PrintWindow(hwnd, *scope_guard_hdc_mem, PRINT_WINDOW_FLAGS(2)).as_bool();
        }

        if !is_success && DwmIsCompositionEnabled()?.as_bool() {
            is_success = PrintWindow(hwnd, *scope_guard_hdc_mem, PRINT_WINDOW_FLAGS(0)).as_bool();
        }

        if !is_success {
            is_success = PrintWindow(hwnd, *scope_guard_hdc_mem, PRINT_WINDOW_FLAGS(4)).as_bool();
        }

        if !is_success {
            is_success = BitBlt(
                *scope_guard_hdc_mem,
                0,
                0,
                width,
                height,
                Some(*scope_guard_hdc_window),
                0,
                0,
                SRCCOPY,
            )
            .is_ok();
        }

        SelectObject(*scope_guard_hdc_mem, previous_object);

        let image = to_rgba_image(*scope_guard_hdc_mem, *scope_guard_h_bitmap, width, height)?;

        let mut rc_client = window_info.rcClient;

        let x = ((rc_client.left - rc_window.left) as f32 * scale_factor).ceil();
        let y = ((rc_client.top - rc_window.top) as f32 * scale_factor).ceil();
        let w = ((rc_client.right - rc_client.left) as f32 * scale_factor).floor();
        let h = ((rc_client.bottom - rc_client.top) as f32 * scale_factor).floor();

        Ok(DynamicImage::ImageRgba8(image)
            .crop(x as u32, y as u32, w as u32, h as u32)
            .to_rgba8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    #[test]
    fn test_capture_monitor() {
        let result = capture_monitor(0, 0, 100, 100);
        assert!(result.is_ok());
        let image = result.unwrap();
        assert_eq!(image.width(), 100);
        assert_eq!(image.height(), 100);
    }

    #[test]
    fn test_capture_window() {
        unsafe {
            let hwnd = GetDesktopWindow();
            let result = capture_window(hwnd, 1.0);
            assert!(result.is_ok());

            let image = result.unwrap();
            assert!(image.width() > 0);
            assert!(image.height() > 0);
        }
    }
}
