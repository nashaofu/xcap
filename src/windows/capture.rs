use crate::{
    XCapError, XCapResult,
    platform::{impl_monitor::ImplMonitor, impl_window::ImplWindow},
};

use image::RgbaImage;

fn check_capture_region(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    monitor_width: u32,
    monitor_height: u32,
) -> XCapResult<()> {
    // Validate region bounds
    if width > monitor_width
        || height > monitor_height
        || x + width > monitor_width
        || y + height > monitor_height
    {
        return Err(XCapError::InvalidCaptureRegion(format!(
            "Region ({x}, {y}, {width}, {height}) is outside monitor size ({monitor_width}, {monitor_height})"
        )));
    }
    Ok(())
}

#[cfg(feature = "wgc")]
pub(super) fn capture_monitor(
    monitor: &ImplMonitor,
    x: Option<u32>,
    y: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
) -> XCapResult<RgbaImage> {
    use super::wgc;
    if let (Some(x), Some(y), Some(width), Some(height)) = (x, y, width, height) {
        let monitor_width = monitor.width()?;
        let monitor_height = monitor.height()?;
        check_capture_region(x, y, width, height, monitor_width, monitor_height)?;
        wgc::capture_monitor(monitor.h_monitor, x, y, width, height)
    } else {
        wgc::capture_monitor(monitor.h_monitor, 0, 0, monitor.width()?, monitor.height()?)
    }
}

#[cfg(not(feature = "wgc"))]
pub(super) fn capture_monitor(
    monitor: &ImplMonitor,
    x: Option<u32>,
    y: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
) -> XCapResult<RgbaImage> {
    use super::gdi;

    let monitor_x = monitor.x()?;
    let monitor_y = monitor.y()?;
    let monitor_width = monitor.width()?;
    let monitor_height = monitor.height()?;

    if let (Some(x), Some(y), Some(width), Some(height)) = (x, y, width, height) {
        check_capture_region(x, y, width, height, monitor_width, monitor_height)?;

        // Calculate absolute coordinates
        let abs_x = monitor_x + x as i32;
        let abs_y = monitor_y + y as i32;

        gdi::capture_monitor(abs_x, abs_y, width as i32, height as i32)
    } else {
        gdi::capture_monitor(
            monitor_x,
            monitor_y,
            monitor_width as i32,
            monitor_height as i32,
        )
    }
}

#[cfg(feature = "wgc")]
pub(super) fn capture_window(window: &ImplWindow) -> XCapResult<RgbaImage> {
    use windows::Win32::System::Threading::{GetCurrentProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    use super::{
        utils::{get_process_is_dpi_awareness, open_process},
        wgc,
    };

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
    let width = (window.width()? as f32 * scale_factor).ceil() as u32;
    let height = (window.height()? as f32 * scale_factor).ceil() as u32;
    wgc::capture_window(window.hwnd, 0, 0, width, height)
}

#[cfg(not(feature = "wgc"))]
pub(super) fn capture_window(window: &ImplWindow) -> XCapResult<RgbaImage> {
    use super::gdi;

    gdi::capture_window(window.hwnd)
}
