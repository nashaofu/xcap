use crate::{
    XCapResult,
    platform::{impl_monitor::ImplMonitor, impl_window::ImplWindow},
};

use super::utils::{get_process_is_dpi_awareness, open_process};
use image::RgbaImage;
use windows::Win32::System::Threading::{GetCurrentProcess, PROCESS_QUERY_LIMITED_INFORMATION};

fn get_scale_factor(window: &ImplWindow) -> XCapResult<f32> {
    // 在win10之后，不同窗口有不同的dpi，所以可能存在截图不全或者截图有较大空白，实际窗口没有填充满图片
    // 如果窗口不感知dpi，那么就不需要缩放，如果当前进程感知dpi，那么也不需要缩放
    let scope_guard_handle = open_process(PROCESS_QUERY_LIMITED_INFORMATION, false, window.pid()?)?;
    let window_is_dpi_awareness = get_process_is_dpi_awareness(*scope_guard_handle)?;
    let current_process_is_dpi_awareness =
        unsafe { get_process_is_dpi_awareness(GetCurrentProcess())? };

    let scale_factor = if !window_is_dpi_awareness || current_process_is_dpi_awareness {
        1.0
    } else {
        window.current_monitor()?.scale_factor()?
    };
    Ok(scale_factor)
}

#[cfg(feature = "wgc")]
pub(super) fn capture_monitor(
    monitor: &ImplMonitor,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    use super::wgc;
    wgc::capture_monitor(monitor.h_monitor, x as u32, y as u32, width, height)
}

#[cfg(not(feature = "wgc"))]
pub(super) fn capture_monitor(
    _monitor: &ImplMonitor,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    use super::gdi;
    gdi::capture_monitor(x, y, width as i32, height as i32)
}

#[cfg(feature = "wgc")]
pub(super) fn capture_window(window: &ImplWindow) -> XCapResult<RgbaImage> {
    use super::wgc;

    let scale_factor = get_scale_factor(window)?;
    let width = (window.width()? as f32 * scale_factor).ceil() as u32;
    let height = (window.height()? as f32 * scale_factor).ceil() as u32;
    wgc::capture_window(window.hwnd, 0, 0, width, height)
}

#[cfg(not(feature = "wgc"))]
pub(super) fn capture_window(window: &ImplWindow) -> XCapResult<RgbaImage> {
    use super::gdi;

    let scale_factor = get_scale_factor(window)?;
    gdi::capture_window(window.hwnd, scale_factor)
}
