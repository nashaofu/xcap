use image::RgbaImage;
use std::env::var_os;

use crate::error::XCapResult;

use super::{
    impl_monitor::ImplMonitor, impl_window::ImplWindow, wayland_capture::wayland_capture,
    xorg_capture::xorg_capture,
};

fn wayland_detect() -> bool {
    let xdg_session_type = var_os("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let wayland_display = var_os("WAYLAND_DISPLAY")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    xdg_session_type.eq("wayland") || wayland_display.to_lowercase().contains("wayland")
}

pub fn capture_monitor(impl_monitor: &ImplMonitor) -> XCapResult<RgbaImage> {
    if wayland_detect() {
        wayland_capture(impl_monitor)
    } else {
        let x = ((impl_monitor.x as f32) * impl_monitor.scale_factor) as i32;
        let y = ((impl_monitor.y as f32) * impl_monitor.scale_factor) as i32;
        let width = ((impl_monitor.width as f32) * impl_monitor.scale_factor) as u32;
        let height = ((impl_monitor.height as f32) * impl_monitor.scale_factor) as u32;

        xorg_capture(impl_monitor.screen_buf.root(), x, y, width, height)
    }
}

pub fn capture_window(impl_window: &ImplWindow) -> XCapResult<RgbaImage> {
    let width = impl_window.width;
    let height = impl_window.height;

    xorg_capture(impl_window.window, 0, 0, width, height)
}

// fn capture_screen_area(
//     screen_info: &ScreenInfo,
//     x: i32,
//     y: i32,
//     width: u32,
//     height: u32,
// ) -> XCapResult<RgbaImage> {
//     if wayland_detect() {
//         wayland_capture_screen_area(screen_info, x, y, width, height)
//     } else {
//         xorg_capture_screen_area(screen_info, x, y, width, height)
//     }
// }
