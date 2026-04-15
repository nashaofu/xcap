use std::mem;

use image::RgbaImage;
use scopeguard::guard;
use windows::Win32::{
    Foundation::{GetLastError, HWND},
    Graphics::Gdi::{
        BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetWindowDC, HBITMAP, HDC, ReleaseDC,
        SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::{
        GetDesktopWindow, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
    },
};

use crate::{
    error::{XCapError, XCapResult},
    platform::utils::get_window_bounds,
};

use super::utils::bgra_to_rgba;

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

    let mut width = window_bounds.right - window_bounds.left;
    let mut height = window_bounds.bottom - window_bounds.top;

    unsafe {
        // Use the desktop window DC and blit the window region by screen coordinates.
        // This avoids PrintWindow which fails for many hardware-accelerated windows
        // (games, WebView2, etc.) and captures whatever is visible on screen.
        let hwnd_desktop = GetDesktopWindow();
        let scope_guard_hdc_desktop = guard(GetWindowDC(Some(hwnd_desktop)), |val| {
            if ReleaseDC(Some(hwnd_desktop), val) != 1 {
                log::error!("ReleaseDC({:?}) failed: {:?}", val, GetLastError());
            }
        });

        // Fallback to screen size if window reports zero dimensions
        if width == 0 {
            width = GetSystemMetrics(SM_CXSCREEN);
        }
        if height == 0 {
            height = GetSystemMetrics(SM_CYSCREEN);
        }

        let scope_guard_hdc_mem = guard(
            CreateCompatibleDC(Some(*scope_guard_hdc_desktop)),
            |val| {
                if !DeleteDC(val).as_bool() {
                    log::error!("DeleteDC({:?}) failed: {:?}", val, GetLastError());
                }
            },
        );
        let scope_guard_h_bitmap = guard(
            CreateCompatibleBitmap(*scope_guard_hdc_desktop, width, height),
            delete_bitmap_object,
        );

        let previous_object = SelectObject(*scope_guard_hdc_mem, (*scope_guard_h_bitmap).into());

        let is_success = BitBlt(
            *scope_guard_hdc_mem,
            0,
            0,
            width,
            height,
            Some(*scope_guard_hdc_desktop),
            window_bounds.left,
            window_bounds.top,
            SRCCOPY,
        )
        .is_ok();

        SelectObject(*scope_guard_hdc_mem, previous_object);

        if !is_success {
            return Err(XCapError::new("Capture window failed"));
        }

        to_rgba_image(*scope_guard_hdc_mem, *scope_guard_h_bitmap, width, height)
    }
}
