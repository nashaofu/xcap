use crate::{
    HdrImage, XCapError, XCapResult,
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
    use super::dxgi::{self, CaptureFrame};
    use super::gdi;

    let monitor_width = monitor.width()?;
    let monitor_height = monitor.height()?;

    let (cap_x, cap_y, cap_w, cap_h) =
        if let (Some(x), Some(y), Some(width), Some(height)) = (x, y, width, height) {
            check_capture_region(x, y, width, height, monitor_width, monitor_height)?;
            (x, y, width, height)
        } else {
            (0, 0, monitor_width, monitor_height)
        };

    // Try DXGI Desktop Duplication first (supports HDR). Fall back to GDI on
    // E_ACCESSDENIED (secure desktop / UAC) or other driver-level failures.
    match dxgi::capture_monitor(monitor.h_monitor, cap_x, cap_y, cap_w, cap_h) {
        Ok(CaptureFrame::Sdr(img)) => Ok(img),
        Ok(CaptureFrame::Hdr(hdr)) => {
            // Tone-map HDR → SDR so existing callers get a usable image.
            Ok(hdr.to_rgba_image_tonemapped(400.0))
        }
        Err(err) => {
            log::debug!("DXGI capture failed ({err}), falling back to GDI");
            let monitor_x = monitor.x()?;
            let monitor_y = monitor.y()?;
            let abs_x = monitor_x + cap_x as i32;
            let abs_y = monitor_y + cap_y as i32;
            gdi::capture_monitor(abs_x, abs_y, cap_w as i32, cap_h as i32)
        }
    }
}

/// Capture the full monitor and return raw HDR pixel data when the monitor is in
/// HDR mode, or an `HdrImage` built from SDR data otherwise.
///
/// Only available without the `wgc` feature (requires DXGI Desktop Duplication).
#[cfg(not(feature = "wgc"))]
pub(super) fn capture_monitor_hdr(monitor: &ImplMonitor) -> XCapResult<HdrImage> {
    use super::dxgi::{self, CaptureFrame};

    let width = monitor.width()?;
    let height = monitor.height()?;

    match dxgi::capture_monitor(monitor.h_monitor, 0, 0, width, height)? {
        CaptureFrame::Hdr(hdr) => Ok(hdr),
        CaptureFrame::Sdr(_) => Err(XCapError::new(
            "DXGI opened in BGRA8 mode despite HDR display; driver does not support R16G16B16A16_FLOAT duplication",
        )),
    }
}

/// Whether the monitor is currently in HDR mode (DXGI Desktop Duplication path).
#[cfg(not(feature = "wgc"))]
pub(super) fn monitor_is_hdr(monitor: &ImplMonitor) -> bool {
    super::dxgi::is_hdr_monitor(monitor.h_monitor)
}

/// HDR capture is not supported on the WGC path (WGC only captures in BGRA8).
#[cfg(feature = "wgc")]
pub(super) fn capture_monitor_hdr(_monitor: &ImplMonitor) -> XCapResult<HdrImage> {
    Err(XCapError::NotSupported)
}

/// HDR detection is not available on the WGC path.
#[cfg(feature = "wgc")]
pub(super) fn monitor_is_hdr(_monitor: &ImplMonitor) -> bool {
    false
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
