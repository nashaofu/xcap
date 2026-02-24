use crate::{
    XCapResult,
    platform::{impl_monitor::ImplMonitor, impl_window::ImplWindow},
};

#[cfg(not(feature = "wgc"))]
use super::gdi;
#[cfg(feature = "wgc")]
use super::wgc;

use cfg_if::cfg_if;
use image::RgbaImage;

pub fn capture_monitor(monitor: &ImplMonitor) -> XCapResult<RgbaImage> {
    cfg_if! {
        if #[cfg(feature = "wgc")] {
            wgc::capture_monitor(monitor.h_monitor)
        } else {
            let x = monitor.x()?;
            let y = monitor.y()?;
            let width = monitor.width()?;
            let height = monitor.height()?;
            gdi::capture_monitor(x as i32, y as i32, width as i32, height as i32)
        }
    }
}

pub fn capture_window(window: &ImplWindow) -> XCapResult<RgbaImage> {
    cfg_if! {
        if #[cfg(feature = "wgc")] {
            wgc::capture_window(window.hwnd)
        } else {
            use windows::Win32::System::Threading::GetCurrentProcess;
            use super::utils::get_process_is_dpi_awareness;
            // 在win10之后，不同窗口有不同的dpi，所以可能存在截图不全或者截图有较大空白，实际窗口没有填充满图片
            // 如果窗口不感知dpi，那么就不需要缩放，如果当前进程感知dpi，那么也不需要缩放
            let scope_guard_handle =
                open_process(PROCESS_QUERY_LIMITED_INFORMATION, false, window.pid()?)?;
            let window_is_dpi_awareness = get_process_is_dpi_awareness(*scope_guard_handle)?;
            let current_process_is_dpi_awareness =
                unsafe { get_process_is_dpi_awareness(GetCurrentProcess())? };

            let scale_factor = if !window_is_dpi_awareness || current_process_is_dpi_awareness {
                1.0
            } else {
                window.current_monitor()?.scale_factor()?
            };
            gdi::capture_window(window.hwnd)
        }
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
