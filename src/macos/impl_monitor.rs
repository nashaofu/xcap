use image::RgbaImage;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayIsActive,
    CGDisplayIsMain, CGDisplayModeGetPixelWidth, CGDisplayModeGetRefreshRate, CGDisplayRotation,
    CGError, CGGetActiveDisplayList, CGGetDisplaysWithPoint, CGWindowListOption,
};
use objc2_foundation::{NSNumber, NSString};

use crate::error::{XCapError, XCapResult};

use super::{capture::capture, impl_video_recorder::ImplVideoRecorder};

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    pub cg_direct_display_id: CGDirectDisplayID,
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

fn get_display_friendly_name(display_id: CGDirectDisplayID) -> XCapResult<String> {
    let screens = NSScreen::screens(unsafe { MainThreadMarker::new_unchecked() });
    for screen in screens {
        let device_description = screen.deviceDescription();
        let screen_number = device_description
            .objectForKey(&NSString::from_str("NSScreenNumber"))
            .ok_or(XCapError::new("Get NSScreenNumber failed"))?;

        let screen_id = screen_number
            .downcast::<NSNumber>()
            .map_err(|err| XCapError::new(format!("{:?}", err)))?
            .unsignedIntValue();

        if screen_id == display_id {
            unsafe { return Ok(screen.localizedName().to_string()) };
        }
    }

    Err(XCapError::new(format!(
        "Get display {} friendly name failed",
        display_id
    )))
}

impl ImplMonitor {
    pub(super) fn new(id: CGDirectDisplayID) -> XCapResult<ImplMonitor> {
        unsafe {
            let CGRect { origin, size } = CGDisplayBounds(id);

            let rotation = CGDisplayRotation(id) as f32;

            let display_mode = CGDisplayCopyDisplayMode(id);
            let pixel_width = CGDisplayModeGetPixelWidth(display_mode.as_deref());
            let scale_factor = pixel_width as f32 / size.width as f32;
            let frequency = CGDisplayModeGetRefreshRate(display_mode.as_deref()) as f32;
            let is_primary = CGDisplayIsMain(id);

            Ok(ImplMonitor {
                cg_direct_display_id: id,
                id,
                name: get_display_friendly_name(id).unwrap_or(format!("Unknown Monitor {}", id)),
                x: origin.x as i32,
                y: origin.y as i32,
                width: size.width as u32,
                height: size.height as u32,
                rotation,
                scale_factor,
                frequency,
                is_primary,
            })
        }
    }
    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        let max_displays: u32 = 16;
        let mut active_displays: Vec<CGDirectDisplayID> = vec![0; max_displays as usize];
        let mut display_count: u32 = 0;

        let cg_error = unsafe {
            CGGetActiveDisplayList(
                max_displays,
                active_displays.as_mut_ptr(),
                &mut display_count,
            )
        };

        if cg_error != CGError::Success {
            return Err(XCapError::new(format!(
                "CGGetActiveDisplayList failed: {:?}",
                cg_error
            )));
        }

        active_displays.truncate(display_count as usize);

        let mut impl_monitors = Vec::with_capacity(active_displays.len());

        for display in active_displays {
            // 运行过程中，如果遇到显示器插拔，可能会导致调用报错
            // 对于报错的情况，就把报错的情况给排除掉
            // https://github.com/nashaofu/xcap/issues/118
            if let Ok(impl_monitor) = ImplMonitor::new(display) {
                impl_monitors.push(impl_monitor);
            } else {
                log::error!("ImplMonitor::new({}) failed", display);
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

        if cg_error != CGError::Success {
            return Err(XCapError::new(format!(
                "CGGetDisplaysWithPoint failed: {:?}",
                cg_error
            )));
        }

        if display_count == 0 {
            return Err(XCapError::new("Monitor not found"));
        }

        if let Some(&display_id) = display_ids.first() {
            if unsafe { !CGDisplayIsActive(display_id) } {
                return Err(XCapError::new("Monitor is not active"));
            }
            println!("display_id: {}", display_id);
            ImplMonitor::new(display_id)
        } else {
            Err(XCapError::new("Monitor not found"))
        }
    }
}

impl ImplMonitor {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        let cg_rect = unsafe { CGDisplayBounds(self.cg_direct_display_id) };

        capture(cg_rect, CGWindowListOption::OptionAll, 0)
    }

    pub fn video_recorder(&self) -> XCapResult<ImplVideoRecorder> {
        ImplVideoRecorder::new()
    }
}
