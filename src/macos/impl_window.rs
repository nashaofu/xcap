use std::ffi::c_void;

use image::RgbaImage;
use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{
    CFArrayGetCount, CFArrayGetValueAtIndex, CFBoolean, CFBooleanGetValue, CFDictionary,
    CFDictionaryGetValue, CFNumber, CFNumberGetValue, CFNumberType, CFString, CGPoint, CGRect,
    CGSize,
};
use objc2_core_graphics::{
    CGDisplayBounds, CGMainDisplayID, CGRectContainsPoint, CGRectIntersectsRect,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSNumber, NSString};

use crate::{error::XCapResult, XCapError};

use super::{capture::capture, impl_monitor::ImplMonitor};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub pid: u32,
    pub current_monitor: ImplMonitor,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_focused: bool,
}

unsafe impl Send for ImplWindow {}

fn get_cf_dictionary_get_value(
    cf_dictionary: &CFDictionary,
    key: &str,
) -> XCapResult<*const c_void> {
    unsafe {
        let cf_dictionary_key = CFString::from_str(key);
        let cf_dictionary_key_ref = cf_dictionary_key.as_ref() as *const CFString;

        let value = CFDictionaryGetValue(cf_dictionary, cf_dictionary_key_ref.cast());

        if value.is_null() {
            return Err(XCapError::new(format!(
                "Get CFDictionary {} value failed",
                key
            )));
        }

        Ok(value)
    }
}

fn get_cf_number_i32_value(cf_dictionary: &CFDictionary, key: &str) -> XCapResult<i32> {
    unsafe {
        let cf_number = get_cf_dictionary_get_value(cf_dictionary, key)? as *const CFNumber;

        let mut value: i32 = 0;
        let is_success = CFNumberGetValue(
            &*cf_number,
            CFNumberType::IntType,
            &mut value as *mut _ as *mut c_void,
        );

        if !is_success {
            return Err(XCapError::new(format!(
                "Get {} CFNumberGetValue failed",
                key
            )));
        }

        Ok(value)
    }
}

fn get_cf_string_value(cf_dictionary: &CFDictionary, key: &str) -> XCapResult<String> {
    let value_ref = get_cf_dictionary_get_value(cf_dictionary, key)? as *const CFString;
    let value = unsafe { (*value_ref).to_string() };
    Ok(value)
}

fn get_cf_bool_value(cf_dictionary: &CFDictionary, key: &str) -> XCapResult<bool> {
    let value_ref = get_cf_dictionary_get_value(cf_dictionary, key)? as *const CFBoolean;

    Ok(unsafe { CFBooleanGetValue(&*value_ref) })
}

fn get_window_cg_rect(window_cf_dictionary: &CFDictionary) -> XCapResult<CGRect> {
    unsafe {
        let window_bounds = get_cf_dictionary_get_value(window_cf_dictionary, "kCGWindowBounds")?
            as *const CFDictionary;

        let mut cg_rect = CGRect::default();

        let is_success =
            CGRectMakeWithDictionaryRepresentation(Some(&*window_bounds), &mut cg_rect);

        if !is_success {
            return Err(XCapError::new(
                "CGRectMakeWithDictionaryRepresentation failed",
            ));
        }

        Ok(cg_rect)
    }
}

impl ImplWindow {
    pub fn new(
        window_cf_dictionary: &CFDictionary,
        impl_monitors: &[ImplMonitor],
        window_name: String,
        window_owner_name: String,
        z: i32,
        focused_app_pid: Option<i32>,
    ) -> XCapResult<ImplWindow> {
        let id = get_cf_number_i32_value(window_cf_dictionary, "kCGWindowNumber")? as u32;
        let pid = get_cf_number_i32_value(window_cf_dictionary, "kCGWindowOwnerPID")?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary)?;

        let primary_monitor = ImplMonitor::new(unsafe { CGMainDisplayID() })?;

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
                .find(|impl_monitor| unsafe {
                    let display_bounds = CGDisplayBounds(impl_monitor.cg_direct_display_id);
                    CGRectContainsPoint(display_bounds, cg_point)
                        || CGRectIntersectsRect(display_bounds, cg_rect)
                })
                .unwrap_or(&primary_monitor);

            (
                cg_rect.size.width as u32 >= impl_monitor.width
                    && cg_rect.size.height as u32 >= impl_monitor.height,
                impl_monitor,
            )
        };

        let is_minimized =
            !get_cf_bool_value(window_cf_dictionary, "kCGWindowIsOnscreen")? && !is_maximized;

        let is_focused = focused_app_pid.eq(&Some(pid));

        Ok(ImplWindow {
            id,
            title: window_name,
            app_name: window_owner_name,
            pid: pid as u32,
            current_monitor: current_monitor.clone(),
            x: cg_rect.origin.x as i32,
            y: cg_rect.origin.y as i32,
            z,
            width: cg_rect.size.width as u32,
            height: cg_rect.size.height as u32,
            is_minimized,
            is_maximized,
            is_focused,
        })
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        unsafe {
            let impl_monitors = ImplMonitor::all()?;
            let workspace = NSWorkspace::sharedWorkspace();
            let pid_key = NSString::from_str("NSApplicationProcessIdentifier");
            let focused_app_pid = workspace
                .activeApplication()
                .and_then(|dictionary| dictionary.valueForKey(&pid_key))
                .and_then(|pid| pid.downcast::<NSNumber>().ok())
                .map(|pid| pid.intValue());

            let mut impl_windows = Vec::new();

            // CGWindowListCopyWindowInfo 返回窗口顺序为从顶层到最底层
            // 即在前面的窗口在数组前面
            let cf_array = match CGWindowListCopyWindowInfo(
                CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
                0,
            ) {
                Some(cf_array) => cf_array,
                None => return Ok(impl_windows),
            };

            let num_windows = CFArrayGetCount(&cf_array);

            for i in 0..num_windows {
                let window_cf_dictionary_ref =
                    CFArrayGetValueAtIndex(&cf_array, i) as *const CFDictionary;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let window_cf_dictionary = &*window_cf_dictionary_ref;

                let window_name = match get_cf_string_value(window_cf_dictionary, "kCGWindowName") {
                    Ok(window_name) => window_name,
                    _ => continue,
                };

                let window_owner_name =
                    match get_cf_string_value(window_cf_dictionary, "kCGWindowOwnerName") {
                        Ok(window_owner_name) => window_owner_name,
                        _ => continue,
                    };

                if window_name.eq("StatusIndicator") && window_owner_name.eq("Window Server") {
                    continue;
                }

                let window_sharing_state =
                    match get_cf_number_i32_value(window_cf_dictionary, "kCGWindowSharingState") {
                        Ok(window_sharing_state) => window_sharing_state as u32,
                        _ => continue,
                    };

                if window_sharing_state == 0 {
                    continue;
                }

                if let Ok(impl_window) = ImplWindow::new(
                    window_cf_dictionary,
                    &impl_monitors,
                    window_name.clone(),
                    window_owner_name.clone(),
                    num_windows as i32 - i as i32 - 1,
                    focused_app_pid,
                ) {
                    impl_windows.push(impl_window);
                } else {
                    log::error!(
                        "ImplWindow::new({:?}, {:?}, {:?}, {:?}) failed",
                        window_cf_dictionary,
                        &impl_monitors,
                        &window_name,
                        &window_owner_name
                    );
                }
            }

            Ok(impl_windows)
        }
    }
}

impl ImplWindow {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture(
            CGRect::new(
                CGPoint::new(self.x as f64, self.y as f64),
                CGSize::new(self.width as f64, self.height as f64),
            ),
            CGWindowListOption::OptionIncludingWindow,
            self.id,
        )
    }
}
