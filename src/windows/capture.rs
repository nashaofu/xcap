use image::RgbaImage;
use windows::Win32::{
    Foundation::HWND, Graphics::Gdi::HMONITOR, UI::WindowsAndMessaging::WINDOWINFO,
};

use crate::{error::XCapResult, platform::wgc::wgc_capture_window};

use super::{
    gdi::{gdi_capture_monitor, gdi_capture_window},
    impl_monitor::ImplMonitor,
    wgc::wgc_capture_monitor,
};

#[allow(unused)]
pub fn capture_monitor(
    hmonitor: HMONITOR,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    if let Ok(image) = wgc_capture_monitor(hmonitor) {
        Ok(image)
    } else {
        gdi_capture_monitor(x, y, width, height)
    }
}

#[allow(unused)]
pub fn capture_window(
    hwnd: HWND,
    pid: u32,
    current_monitor: &ImplMonitor,
    window_info: &WINDOWINFO,
) -> XCapResult<RgbaImage> {
    if let Ok(image) = wgc_capture_window(hwnd) {
        Ok(image)
    } else {
        gdi_capture_window(hwnd, pid, current_monitor, window_info)
    }
}
