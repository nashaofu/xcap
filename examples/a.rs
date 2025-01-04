use image::{ImageBuffer, Rgba, RgbaImage};
use std::{thread, time::Duration};
use windows::{
    core::w,
    Win32::{
        Foundation::{BOOL, HWND, LPARAM, RECT, TRUE},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
            GetDIBits, GetWindowDC, ReleaseDC, SelectObject, StretchDIBits, BITMAPINFO, BI_RGB,
            DIB_RGB_COLORS, HRGN, SRCCOPY,
        },
        UI::{
            Magnification::{
                MagInitialize, MagSetImageScalingCallback, MagSetWindowFilterList,
                MagSetWindowSource, MagSetWindowTransform, MagUninitialize, MAGIMAGEHEADER,
                MAGTRANSFORM, MW_FILTERMODE_EXCLUDE, MW_FILTERMODE_INCLUDE, WC_MAGNIFIER,
            },
            WindowsAndMessaging::{
                CreateWindowExW, EnumWindows, GetClientRect, WS_CHILD, WS_DISABLED,
                WS_EX_CLIENTEDGE, WS_EX_LAYERED, WS_EX_LAYOUTRTL, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
                WS_MAXIMIZE, WS_MINIMIZE, WS_VISIBLE,
            },
        },
    },
};

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, state: LPARAM) -> BOOL {
    let state = Box::leak(Box::from_raw(state.0 as *mut Vec<HWND>));
    state.push(hwnd);
    TRUE
}

unsafe extern "system" fn image_scaling_callback(
    hwnd: HWND,
    srcdata: *mut std::ffi::c_void,
    srcheader: MAGIMAGEHEADER,
    destdata: *mut std::ffi::c_void,
    destheader: MAGIMAGEHEADER,
    src_rect: RECT,
    dest_rect: RECT,
    dirty: HRGN,
) -> BOOL {
    // 创建与目标尺寸一致的位图
    let hdc = GetWindowDC(HWND(120680 as _));
    let hdc_mem = CreateCompatibleDC(hdc);
    let s = CreateCompatibleBitmap(
        hdc,
        dest_rect.right - dest_rect.left,
        dest_rect.bottom - dest_rect.top,
    );
    SelectObject(hdc_mem, s);
    let width = src_rect.right - src_rect.left;
    let height = src_rect.bottom - src_rect.top;
    let buffer_size = width * height * 4;

    // 将放大内容拷贝到位图中
    StretchDIBits(
        hdc_mem,
        0,
        0,
        srcheader.width as i32,
        srcheader.height as i32,
        0,
        0,
        srcheader.width as i32,
        srcheader.height as i32,
        Some(srcdata),
        &srcheader as *const _ as *const BITMAPINFO,
        windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
        SRCCOPY,
    );

    println!("width: {:?}", width);
    DeleteDC(hdc_mem).unwrap();
    ReleaseDC(None, hdc);

    println!("image_scaling_callback {:?}", destdata);

    let src_slice = std::slice::from_raw_parts(destdata as *const u8, buffer_size as usize);
    let src_vec = src_slice.to_vec();
    let s = RgbaImage::from_raw(width as u32, height as u32, src_vec).unwrap();

    s.save("screenshot2.png").unwrap();

    BOOL(1)
}

fn capture_with_magnifier(hwnd: HWND) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    unsafe {
        // 初始化放大镜
        if !MagInitialize().as_bool() {
            return None;
        }

        let hdc_window = GetWindowDC(hwnd);
        if hdc_window.is_invalid() {
            return None;
        }

        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return None;
        }

        let magnifier_win: HWND = CreateWindowExW(
            WS_EX_TOPMOST,
            WC_MAGNIFIER,
            w!("Magnifier"),
            WS_CHILD,
            0,
            0,
            2560,
            1440,
            hwnd,
            None,
            None,
            None,
        )
        .unwrap();

        // 设置放大镜变换
        let mut mag_transform = MAGTRANSFORM {
            v: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        };

        // (HWND, i32) 表示当前窗口以及层级，既（窗口，层级 z），i32 表示 max_z_order，既最大的窗口的 z 顺序
        // 窗口当前层级为 max_z_order - z
        let hwnds_mut_ptr: *mut Vec<HWND> = Box::into_raw(Box::default());

        let mut hwnds = {
            // EnumWindows 函数按照 Z 顺序遍历顶层窗口，从最顶层的窗口开始，依次向下遍历。
            EnumWindows(Some(enum_windows_proc), LPARAM(hwnds_mut_ptr as isize)).unwrap();
            let s = Box::from_raw(hwnds_mut_ptr);
            let hwnds = s
                .iter()
                .filter(|&x| !x.eq(&hwnd))
                .map(|hwnd| *hwnd)
                .collect::<Vec<_>>();
            hwnds
        };

        println!("hwnds: {:?}", hwnds);

        MagSetWindowFilterList(magnifier_win, MW_FILTERMODE_EXCLUDE, 2, hwnds.as_mut_ptr())
            .unwrap();

        MagSetWindowTransform(magnifier_win, &mut mag_transform).unwrap();

        MagSetImageScalingCallback(magnifier_win, Some(image_scaling_callback)).unwrap();

        // 设置放大镜窗口源
        let mag_rect = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };

        let s = MagSetWindowSource(magnifier_win, mag_rect).ok().unwrap();
        None
    }
}

fn main() {
    let hwnd = HWND(2492384 as _);
    if let Some(image) = capture_with_magnifier(hwnd) {
        image.save("screenshot.png").unwrap();
        println!("截图已保存为 screenshot.png");
    } else {
        println!("无法截取窗口截图");
    }
}
