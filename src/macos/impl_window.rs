use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetValueAtIndex},
    base::{FromVoid, TCFType},
    dictionary::{CFDictionaryGetValue, CFDictionaryRef},
    number::{kCFNumberIntType, CFBooleanGetValue, CFBooleanRef, CFNumberGetValue, CFNumberRef},
    string::CFString,
};
use core_graphics::{
    display::{
        kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
        kCGWindowListOptionOnScreenOnly, CGPoint, CGWindowListCopyWindowInfo,
    },
    geometry::CGRect,
    window::kCGWindowSharingNone,
};
use image::RgbaImage;
use std::ffi::c_void;

use crate::error::XCapResult;

use super::{capture::capture, impl_monitor::ImplMonitor};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub window_cf_dictionary_ref: CFDictionaryRef,
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub current_monitor: ImplMonitor,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_maximized: bool,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGRectMakeWithDictionaryRepresentation(
        dict: CFDictionaryRef,
        rect: &mut CGRect,
    ) -> CFBooleanRef;
}

fn get_cf_dictionary_get_value(cf_dictionary_ref: CFDictionaryRef, key: &str) -> *const c_void {
    unsafe {
        let cf_dictionary_key = CFString::new(key);

        CFDictionaryGetValue(cf_dictionary_ref, cf_dictionary_key.as_CFTypeRef())
    }
}

fn get_window_cg_rect(window_cf_dictionary_ref: CFDictionaryRef) -> CGRect {
    unsafe {
        let window_bounds_ref =
            get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowBounds")
                as CFDictionaryRef;

        let mut cg_rect = CGRect::default();
        CGRectMakeWithDictionaryRepresentation(window_bounds_ref, &mut cg_rect);
        cg_rect
    }
}

impl ImplWindow {
    pub fn new(
        window_cf_dictionary_ref: CFDictionaryRef,
        impl_monitors: &[ImplMonitor],
    ) -> XCapResult<ImplWindow> {
        unsafe {
            let id = {
                let cf_number_ref =
                    get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowNumber")
                        as CFNumberRef;

                let mut window_id: u32 = 0;
                CFNumberGetValue(
                    cf_number_ref,
                    kCFNumberIntType,
                    &mut window_id as *mut _ as *mut c_void,
                );
                window_id
            };

            let title = {
                let window_title_ref =
                    get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowName");
                CFString::from_void(window_title_ref).to_string()
            };

            let app_name = {
                let window_owner_name_ref =
                    get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowOwnerName");
                CFString::from_void(window_owner_name_ref).to_string()
            };

            let cg_rect = get_window_cg_rect(window_cf_dictionary_ref);

            let is_minimized = {
                let window_is_on_screen_ref =
                    get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowIsOnscreen");
                !CFBooleanGetValue(window_is_on_screen_ref as CFBooleanRef)
            };

            let (is_maximized, current_monitor) = {
                // 获取窗口中心点的坐标
                let window_center_x = cg_rect.origin.x + cg_rect.size.width / 2.0;
                let window_center_y = cg_rect.origin.y + cg_rect.size.height / 2.0;
                let cg_point = CGPoint {
                    x: window_center_x,
                    y: window_center_y,
                };

                let impl_monitor = impl_monitors
                    .iter()
                    .find(|impl_monitor| {
                        let display_bounds = impl_monitor.cg_display.bounds();
                        display_bounds.contains(&cg_point) || display_bounds.is_intersects(&cg_rect)
                    })
                    .unwrap_or(&impl_monitors[0]);

                (
                    cg_rect.size.width as u32 >= impl_monitor.width
                        && cg_rect.size.height as u32 >= impl_monitor.height,
                    impl_monitor,
                )
            };

            Ok(ImplWindow {
                window_cf_dictionary_ref,
                id,
                title,
                app_name,
                current_monitor: current_monitor.clone(),
                x: cg_rect.origin.x as i32,
                y: cg_rect.origin.y as i32,
                width: cg_rect.size.width as u32,
                height: cg_rect.size.height as u32,
                is_minimized,
                is_maximized,
            })
        }
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        let impl_monitors = ImplMonitor::all()?;
        unsafe {
            let cg_window_list_copy_window_info = CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            );
            let num_windows = CFArrayGetCount(cg_window_list_copy_window_info);

            let mut impl_windows = Vec::new();

            for i in 0..num_windows {
                let window_cf_dictionary_ref =
                    CFArrayGetValueAtIndex(cg_window_list_copy_window_info, i) as CFDictionaryRef;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let window_sharing_state_ref =
                    get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowSharingState");

                let mut window_sharing_state: u32 = 0;
                CFNumberGetValue(
                    window_sharing_state_ref as CFNumberRef,
                    kCFNumberIntType,
                    &mut window_sharing_state as *mut _ as *mut c_void,
                );

                if window_sharing_state == kCGWindowSharingNone {
                    continue;
                }

                impl_windows.push(ImplWindow::new(window_cf_dictionary_ref, &impl_monitors)?);
            }

            Ok(impl_windows)
        }
    }
}

impl ImplWindow {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture(
            get_window_cg_rect(self.window_cf_dictionary_ref),
            kCGWindowListOptionIncludingWindow,
            self.id,
        )
    }
}
