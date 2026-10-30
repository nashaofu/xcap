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
            impl_monitor.x()?,
            impl_monitor.y()?,
            i32::try_from(impl_monitor.width()?).map_err(crate::XCapError::new)?,
            i32::try_from(impl_monitor.height()?).map_err(crate::XCapError::new)?,
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
        let abs_x = i32::try_from(i64::from(impl_monitor.x()?) + i64::from(x))
            .map_err(crate::XCapError::new)?;
        let abs_y = i32::try_from(i64::from(impl_monitor.y()?) + i64::from(y))
            .map_err(crate::XCapError::new)?;
        wayland_capture(
            abs_x,
            abs_y,
            i32::try_from(width).map_err(crate::XCapError::new)?,
            i32::try_from(height).map_err(crate::XCapError::new)?,
        )
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
