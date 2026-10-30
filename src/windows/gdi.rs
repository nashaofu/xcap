use std::{ffi::c_void, mem};

use image::{DynamicImage, RgbaImage, imageops::FilterType};
use scopeguard::guard;
use widestring::U16CString;
use windows::Win32::{
    Foundation::{GetLastError, HWND, MAX_PATH, RECT},
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
    UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetWindowInfo, GetWindowRect, WINDOWINFO,
    },
};

use crate::{
    error::{XCapError, XCapResult},
    platform::utils::get_window_bounds,
};

use super::utils::{bgra_to_rgba, get_os_major_version};

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

    RgbaImage::from_raw(width as u32, height as u32, bgra_to_rgba(buffer))
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
}

fn delete_bitmap_object(val: HBITMAP) {
    unsafe {
        let succeed = DeleteObject(val.into()).as_bool();

        if !succeed {
            log::error!("DeleteObject({:?}) failed: {:?}", val, GetLastError());
        }
    }
}

pub(super) fn capture_monitor(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    unsafe {
        let hwnd = GetDesktopWindow();
        let scope_guard_hdc_desktop_window = guard(GetWindowDC(Some(hwnd)), |val| {
            if ReleaseDC(Some(hwnd), val) != 1 {
                log::error!("ReleaseDC({:?}) failed: {:?}", val, GetLastError());
            }
        });

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

        SelectObject(*scope_guard_mem, (*scope_guard_h_bitmap).into());

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

pub(super) fn capture_window(hwnd: HWND) -> XCapResult<RgbaImage> {
    let window_bounds = get_window_bounds(hwnd)?;

    let window_width = window_bounds.right - window_bounds.left;
    let window_height = window_bounds.bottom - window_bounds.top;
    unsafe {
        let mut bitmap_width = window_width;
        let mut bitmap_height = window_height;

        let scope_guard_hdc_window = guard(GetWindowDC(Some(hwnd)), |val| {
            if ReleaseDC(Some(hwnd), val) != 1 {
                log::error!("ReleaseDC({:?}) failed: {:?}", val, GetLastError());
            }
        });

        let hgdi_obj = GetCurrentObject(*scope_guard_hdc_window, OBJ_BITMAP);
        let mut bitmap = BITMAP::default();

        if GetObjectW(
            hgdi_obj,
            mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut c_void),
        ) != 0
        {
            bitmap_width = bitmap.bmWidth;
            bitmap_height = bitmap.bmHeight;
        }

        let scope_guard_hdc_mem = guard(CreateCompatibleDC(Some(*scope_guard_hdc_window)), |val| {
            if !DeleteDC(val).as_bool() {
                log::error!("DeleteDC({:?}) failed: {:?}", val, GetLastError());
            }
        });
        let scope_guard_h_bitmap = guard(
            CreateCompatibleBitmap(*scope_guard_hdc_window, bitmap_width, bitmap_height),
            delete_bitmap_object,
        );

        let previous_object = SelectObject(*scope_guard_hdc_mem, (*scope_guard_h_bitmap).into());

        let mut is_success = false;

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
                bitmap_width,
                bitmap_height,
                Some(*scope_guard_hdc_window),
                0,
                0,
                SRCCOPY,
            )
            .is_ok();
        }

        if !is_success {
            return Err(XCapError::new("Capture window failed"));
        }

        SelectObject(*scope_guard_hdc_mem, previous_object);

        let image = to_rgba_image(
            *scope_guard_hdc_mem,
            *scope_guard_h_bitmap,
            bitmap_width,
            bitmap_height,
        )?;

        let mut window_rect = RECT::default();
        GetWindowRect(hwnd, &mut window_rect)?;

        let scale_factor = bitmap_width as f32 / window_width as f32;
        // 桌面窗口管理器（DWM）会为部分窗口添加不可见的 resize 边框（通常 8px 左右），
        // PrintWindow 会将这个边框也一起截图，所以需要裁剪掉这个区域
        let mut x = (window_bounds.left - window_rect.left) as f32 * scale_factor;
        let mut y = (window_bounds.top - window_rect.top) as f32 * scale_factor;

        let mut window_info = WINDOWINFO::default();
        GetWindowInfo(hwnd, &mut window_info)?;

        let mut lp_class_name = [0u16; MAX_PATH as usize];
        let lp_class_name_length = GetClassNameW(hwnd, &mut lp_class_name) as usize;

        let class_name = U16CString::from_vec_truncate(&lp_class_name[0..lp_class_name_length])
            .to_string()
            .unwrap_or_default();

        // #32770 为标准 Dialog，DWM 虽然会为它添加 resize 边框，但 PrintWindow 却不会将边框一起截图，所以不需要裁剪
        if class_name == "#32770" {
            x = 0.0;
            y = 0.0;
        }

        Ok(DynamicImage::ImageRgba8(image)
            .resize(
                window_width as u32,
                window_height as u32,
                FilterType::CatmullRom,
            )
            .crop(
                x as u32,
                y as u32,
                window_width as u32,
                window_height as u32,
            )
            .to_rgba8())
    }
}
