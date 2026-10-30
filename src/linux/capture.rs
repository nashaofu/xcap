use image::RgbaImage;

use crate::error::XCapResult;

use super::{
    impl_monitor::ImplMonitor,
    impl_window::ImplWindow,
    utils::{get_current_screen_buf, get_monitor_info_buf, wayland_detect},
    wayland_capture::wayland_capture,
    xorg_capture::xorg_capture,
};

pub fn capture_monitor(impl_monitor: &ImplMonitor) -> XCapResult<RgbaImage> {
    let monitor_info_buf = get_monitor_info_buf(impl_monitor.output)?;

    if wayland_detect() {
        wayland_capture(
            monitor_info_buf.x() as i32,
            monitor_info_buf.y() as i32,
            monitor_info_buf.width() as i32,
            monitor_info_buf.height() as i32,
        )
    } else {
        let screen_buf = get_current_screen_buf()?;

        xorg_capture(
            screen_buf.root(),
            monitor_info_buf.x() as i32,
            monitor_info_buf.y() as i32,
            monitor_info_buf.width() as u32,
            monitor_info_buf.height() as u32,
        )
    }
}

pub fn capture_region(
    impl_monitor: &ImplMonitor,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let monitor_info_buf = get_monitor_info_buf(impl_monitor.output)?;

    if wayland_detect() {
        wayland_capture(x as i32, y as i32, width as i32, height as i32)
    } else {
        let screen_buf = get_current_screen_buf()?;

        xorg_capture(
            screen_buf.root(),
            monitor_info_buf.x() as i32 + x as i32,
            monitor_info_buf.y() as i32 + y as i32,
            width,
            height,
        )
    }
}

pub fn capture_window(impl_window: &ImplWindow) -> XCapResult<RgbaImage> {
    let width = impl_window.width()?;
    let height = impl_window.height()?;

    xorg_capture(impl_window.window, 0, 0, width, height)
}
