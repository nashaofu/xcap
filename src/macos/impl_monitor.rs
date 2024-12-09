use core_graphics::display::{
    kCGNullWindowID, kCGWindowListOptionAll, CGDirectDisplayID, CGDisplay, CGDisplayMode, CGError,
    CGPoint,
};
use image::RgbaImage;

use crate::error::{XCapError, XCapResult};

use super::capture::{capture, capture_bytes};

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    pub cg_display: CGDisplay,
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rotation: f32,
    pub scale_factor: f32,
    pub frequency: f32,
    pub is_primary: bool,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetDisplaysWithPoint(
        point: CGPoint,
        max_displays: u32,
        displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> CGError;
}

impl ImplMonitor {
    pub(super) fn new(id: CGDirectDisplayID) -> XCapResult<ImplMonitor> {
        let cg_display = CGDisplay::new(id);
        let screen_num = cg_display.model_number();
        let cg_rect = cg_display.bounds();
        let cg_display_mode = get_cg_display_mode(cg_display)?;
        let pixel_width = cg_display_mode.pixel_width();
        let scale_factor = pixel_width as f32 / cg_rect.size.width as f32;

        Ok(ImplMonitor {
            cg_display,
            id: cg_display.id,
            name: format!("Monitor #{screen_num}"),
            x: cg_rect.origin.x as i32,
            y: cg_rect.origin.y as i32,
            width: cg_rect.size.width as u32,
            height: cg_rect.size.height as u32,
            rotation: cg_display.rotation() as f32,
            scale_factor,
            frequency: cg_display_mode.refresh_rate() as f32,
            is_primary: cg_display.is_main(),
        })
    }
    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        // active vs online https://developer.apple.com/documentation/coregraphics/1454964-cggetonlinedisplaylist?language=objc
        let display_ids = CGDisplay::active_displays()?;

        let mut impl_monitors: Vec<ImplMonitor> = Vec::with_capacity(display_ids.len());

        for display_id in display_ids {
            // 运行过程中，如果遇到显示器插拔，可能会导致调用报错
            // 对于报错的情况，就把报错的情况给排除掉
            // https://github.com/nashaofu/xcap/issues/118
            if let Ok(impl_monitor) = ImplMonitor::new(display_id) {
                impl_monitors.push(impl_monitor);
            } else {
                log::error!("ImplMonitor::new({}) failed", display_id);
            }
        }

        Ok(impl_monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<ImplMonitor> {
        let point = CGPoint {
            x: x as f64,
            y: y as f64,
        };
        let max_displays: u32 = 16;
        let mut display_ids: Vec<CGDirectDisplayID> = vec![0; max_displays as usize];
        let mut display_count: u32 = 0;

        let cg_error = unsafe {
            CGGetDisplaysWithPoint(
                point,
                max_displays,
                display_ids.as_mut_ptr(),
                &mut display_count,
            )
        };

        if cg_error != 0 {
            return Err(XCapError::CoreGraphicsDisplayCGError(cg_error));
        }

        if display_count == 0 {
            return Err(XCapError::new("Get displays from point failed"));
        }

        let display_id = display_ids
            .first()
            .ok_or(XCapError::new("Monitor not found"))?;

        let impl_monitor = ImplMonitor::new(*display_id)?;

        if !impl_monitor.cg_display.is_active() {
            Err(XCapError::new("Monitor is not active"))
        } else {
            Ok(impl_monitor)
        }
    }
}

fn get_cg_display_mode(cg_display: CGDisplay) -> XCapResult<CGDisplayMode> {
    let cg_display_mode = cg_display
        .display_mode()
        .ok_or_else(|| XCapError::new("Get display mode failed"))?;

    Ok(cg_display_mode)
}

impl ImplMonitor {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture(
            self.cg_display.bounds(),
            kCGWindowListOptionAll,
            kCGNullWindowID,
        )
    }

    pub fn capture_bytes(&self) -> XCapResult<Vec<u8>> {
        capture_bytes(
            self.cg_display.bounds(),
            kCGWindowListOptionAll,
            kCGNullWindowID,
        )
    }
}
