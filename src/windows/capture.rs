use image::RgbaImage;
use std::{ffi::c_void, mem};
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Dwm::DwmIsCompositionEnabled,
        Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, GetCurrentObject, GetDIBits,
            GetObjectW, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
            OBJ_BITMAP, SRCCOPY,
        },
    },
    Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS},
    UI::WindowsAndMessaging::GetDesktopWindow,
};

use crate::{
    error::{XCapError, XCapResult},
    platform::utils::get_window_rect,
};

use super::{
    boxed::{BoxHBITMAP, BoxHDC},
    utils::get_os_major_version,
};

fn to_rgba_image(
    box_hdc_mem: BoxHDC,
    box_h_bitmap: BoxHBITMAP,
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
        let is_success = GetDIBits(
            *box_hdc_mem,
            *box_h_bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        ) == 0;

        if is_success {
            return Err(XCapError::new("Get RGBA data failed"));
        }
    };

    let is_old_version = get_os_major_version() < 8;
    for src in buffer.chunks_exact_mut(4) {
        src.swap(0, 2);
        // fix https://github.com/nashaofu/xcap/issues/92#issuecomment-1910014951
        if src[3] == 0 && is_old_version {
            src[3] = 255;
        }
    }

    RgbaImage::from_raw(width as u32, height as u32, buffer)
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
}

#[allow(unused)]
pub fn capture_monitor(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    unsafe {
        let hwnd = GetDesktopWindow();
        let box_hdc_desktop_window = BoxHDC::from(hwnd);

        // 内存中的HDC，使用 DeleteDC 函数释放
        // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-createcompatibledc
        let box_hdc_mem = BoxHDC::new(CreateCompatibleDC(*box_hdc_desktop_window), None);
        let box_h_bitmap = BoxHBITMAP::new(CreateCompatibleBitmap(
            *box_hdc_desktop_window,
            width,
            height,
        ));

        // 使用SelectObject函数将这个位图选择到DC中
        SelectObject(*box_hdc_mem, *box_h_bitmap);

        // 拷贝原始图像到内存
        // 这里不需要缩放图片，所以直接使用BitBlt
        // 如需要缩放，则使用 StretchBlt
        BitBlt(
            *box_hdc_mem,
            0,
            0,
            width,
            height,
            *box_hdc_desktop_window,
            x,
            y,
            SRCCOPY,
        )?;

        to_rgba_image(box_hdc_mem, box_h_bitmap, width, height)
    }
}

#[allow(unused)]
pub fn capture_window(hwnd: HWND, scale_factor: f32) -> XCapResult<RgbaImage> {
    unsafe {
        let box_hdc_window: BoxHDC = BoxHDC::from(hwnd);
        let rect = get_window_rect(hwnd)?;
        let mut width = rect.right - rect.left;
        let mut height = rect.bottom - rect.top;

        let hgdi_obj = GetCurrentObject(*box_hdc_window, OBJ_BITMAP);
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

        width = (width as f32 * scale_factor) as i32;
        height = (height as f32 * scale_factor) as i32;

        // 内存中的HDC，使用 DeleteDC 函数释放
        // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-createcompatibledc
        let box_hdc_mem = BoxHDC::new(CreateCompatibleDC(*box_hdc_window), None);
        let box_h_bitmap = BoxHBITMAP::new(CreateCompatibleBitmap(*box_hdc_window, width, height));

        let previous_object = SelectObject(*box_hdc_mem, *box_h_bitmap);

        let mut is_success = false;

        // https://webrtc.googlesource.com/src.git/+/refs/heads/main/modules/desktop_capture/win/window_capturer_win_gdi.cc#301
        if get_os_major_version() >= 8 {
            is_success = PrintWindow(hwnd, *box_hdc_mem, PRINT_WINDOW_FLAGS(2)).as_bool();
        }

        if !is_success && DwmIsCompositionEnabled()?.as_bool() {
            is_success = PrintWindow(hwnd, *box_hdc_mem, PRINT_WINDOW_FLAGS(0)).as_bool();
        }

        if !is_success {
            is_success = PrintWindow(hwnd, *box_hdc_mem, PRINT_WINDOW_FLAGS(3)).as_bool();
        }

        if !is_success {
            is_success = BitBlt(
                *box_hdc_mem,
                0,
                0,
                width,
                height,
                *box_hdc_window,
                0,
                0,
                SRCCOPY,
            )
            .is_ok();
        }

        SelectObject(*box_hdc_mem, previous_object);

        to_rgba_image(box_hdc_mem, box_h_bitmap, width, height)
    }
}
