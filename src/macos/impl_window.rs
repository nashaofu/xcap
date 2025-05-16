use std::ffi::c_void;

use image::RgbaImage;
use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CGPoint, CGRect,
};
use objc2_core_graphics::{
    CGDisplayBounds, CGMainDisplayID, CGRectContainsPoint, CGRectIntersectsRect,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSNumber, NSString};

use crate::{XCapError, error::XCapResult};

use super::{capture::capture, impl_monitor::ImplMonitor};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub window_id: u32,
}

unsafe impl Send for ImplWindow {}

fn get_cf_dictionary_get_value(
    cf_dictionary: &CFDictionary,
    key: &str,
) -> XCapResult<*const c_void> {
    unsafe {
        let cf_dictionary_key = CFString::from_str(key);
        let cf_dictionary_key_ref = cf_dictionary_key.as_ref() as *const CFString;

        let value = cf_dictionary.value(cf_dictionary_key_ref.cast());

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
        let is_success =
            (*cf_number).value(CFNumberType::IntType, &mut value as *mut _ as *mut c_void);

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

    Ok(unsafe { (*value_ref).value() })
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

fn get_window_id(window_cf_dictionary: &CFDictionary) -> XCapResult<u32> {
    let window_name = get_cf_string_value(window_cf_dictionary, "kCGWindowName")?;

    let window_owner_name = get_cf_string_value(window_cf_dictionary, "kCGWindowOwnerName")?;

    if window_name.eq("StatusIndicator") && window_owner_name.eq("Window Server") {
        return Err(XCapError::new("Window is StatusIndicator"));
    }

    let window_sharing_state =
        get_cf_number_i32_value(window_cf_dictionary, "kCGWindowSharingState")?;

    if window_sharing_state == 0 {
        return Err(XCapError::new("Window sharing state is 0"));
    }

    let window_id = get_cf_number_i32_value(window_cf_dictionary, "kCGWindowNumber")?;

    Ok(window_id as u32)
}

pub fn get_window_cf_dictionary(window_id: u32) -> XCapResult<CFRetained<CFDictionary>> {
    unsafe {
        // CGWindowListCopyWindowInfo 返回窗口顺序为从顶层到最底层
        // 即在前面的窗口在数组前面
        let cf_array = match CGWindowListCopyWindowInfo(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            0,
        ) {
            Some(cf_array) => cf_array,
            None => return Err(XCapError::new("Get window info failed")),
        };

        let windows_count = cf_array.count();

        for i in 0..windows_count {
            let window_cf_dictionary_ref = cf_array.value_at_index(i) as *const CFDictionary;

            if window_cf_dictionary_ref.is_null() {
                continue;
            }
            let window_cf_dictionary = &*window_cf_dictionary_ref;

            let current_window_id = match get_window_id(window_cf_dictionary) {
                Ok(val) => val,
                Err(_) => continue,
            };

            if current_window_id == window_id {
                let s = CFDictionary::new_copy(None, Some(window_cf_dictionary)).unwrap();
                return Ok(s);
            }
        }

        Err(XCapError::new("Window not found"))
    }
}

impl ImplWindow {
    pub fn new(window_id: u32) -> ImplWindow {
        ImplWindow { window_id }
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        unsafe {
            let mut impl_window = Vec::new();

            // CGWindowListCopyWindowInfo 返回窗口顺序为从顶层到最底层
            // 即在前面的窗口在数组前面
            let cf_array = match CGWindowListCopyWindowInfo(
                CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
                0,
            ) {
                Some(cf_array) => cf_array,
                None => return Ok(impl_window),
            };

            let windows_count = cf_array.count();

            for i in 0..windows_count {
                let window_cf_dictionary_ref = cf_array.value_at_index(i) as *const CFDictionary;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let window_cf_dictionary = &*window_cf_dictionary_ref;

                let window_id = match get_window_id(window_cf_dictionary) {
                    Ok(window_id) => window_id,
                    Err(_) => continue,
                };

                impl_window.push(ImplWindow::new(window_id));
            }

            Ok(impl_window)
        }
    }
}

impl ImplWindow {
    pub fn id(&self) -> XCapResult<u32> {
        Ok(self.window_id)
    }

    pub fn pid(&self) -> XCapResult<u32> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let pid = get_cf_number_i32_value(window_cf_dictionary.as_ref(), "kCGWindowOwnerPID")?;

        Ok(pid as u32)
    }

    pub fn app_name(&self) -> XCapResult<String> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        get_cf_string_value(window_cf_dictionary.as_ref(), "kCGWindowOwnerName")
    }

    pub fn title(&self) -> XCapResult<String> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        get_cf_string_value(window_cf_dictionary.as_ref(), "kCGWindowName")
    }

    pub fn current_monitor(&self) -> XCapResult<ImplMonitor> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;
        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        // 获取窗口中心点的坐标
        let window_center_x = cg_rect.origin.x + cg_rect.size.width / 2.0;
        let window_center_y = cg_rect.origin.y + cg_rect.size.height / 2.0;
        let cg_point = CGPoint {
            x: window_center_x,
            y: window_center_y,
        };

        let impl_monitors = ImplMonitor::all()?;
        let primary_monitor = ImplMonitor::new(unsafe { CGMainDisplayID() });

        let impl_monitor = impl_monitors
            .iter()
            .find(|impl_monitor| unsafe {
                let display_bounds = CGDisplayBounds(impl_monitor.cg_direct_display_id);
                CGRectContainsPoint(display_bounds, cg_point)
                    || CGRectIntersectsRect(display_bounds, cg_rect)
            })
            .unwrap_or(&primary_monitor);

        Ok(impl_monitor.to_owned())
    }

    pub fn x(&self) -> XCapResult<i32> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        Ok(cg_rect.origin.x as i32)
    }

    pub fn y(&self) -> XCapResult<i32> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        Ok(cg_rect.origin.y as i32)
    }

    pub fn z(&self) -> XCapResult<i32> {
        unsafe {
            // CGWindowListCopyWindowInfo 返回窗口顺序为从顶层到最底层
            // 即在前面的窗口在数组前面
            let cf_array = match CGWindowListCopyWindowInfo(
                CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
                0,
            ) {
                Some(cf_array) => cf_array,
                None => return Err(XCapError::new("Get window list failed")),
            };

            let windows_count = cf_array.count();
            let mut z = windows_count as i32;

            for i in 0..windows_count {
                z -= 1;
                let window_cf_dictionary_ref = cf_array.value_at_index(i) as *const CFDictionary;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let window_cf_dictionary = &*window_cf_dictionary_ref;

                let window_id = match get_window_id(window_cf_dictionary) {
                    Ok(window_id) => window_id,
                    Err(_) => continue,
                };

                if window_id == self.window_id {
                    break;
                }
            }

            Ok(z)
        }
    }

    pub fn width(&self) -> XCapResult<u32> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        Ok(cg_rect.size.width as u32)
    }

    pub fn height(&self) -> XCapResult<u32> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        Ok(cg_rect.size.height as u32)
    }

    pub fn is_minimized(&self) -> XCapResult<bool> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;
        let is_on_screen = get_cf_bool_value(window_cf_dictionary.as_ref(), "kCGWindowIsOnscreen")?;
        let is_maximized = self.is_maximized()?;

        Ok(!is_on_screen && !is_maximized)
    }

    pub fn is_maximized(&self) -> XCapResult<bool> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;
        let impl_monitor = self.current_monitor()?;
        let impl_monitor_width = impl_monitor.width()?;
        let impl_monitor_height = impl_monitor.height()?;

        let is_maximized = {
            cg_rect.size.width as u32 >= impl_monitor_width
                && cg_rect.size.height as u32 >= impl_monitor_height
        };

        Ok(is_maximized)
    }

    pub fn is_focused(&self) -> XCapResult<bool> {
        let pid_key = NSString::from_str("NSApplicationProcessIdentifier");

        unsafe {
            let workspace = NSWorkspace::sharedWorkspace();

            // activeApplication is deprecated, but the alternative, frontmostApplication,
            // returns the application in focus when the process started while activeApplication
            // returns a `NSDictionary` of application currently in focus, in real-time
            let active_app_dictionary = workspace.activeApplication();

            let active_app_pid = active_app_dictionary
                .and_then(|dict| dict.valueForKey(&pid_key))
                .and_then(|pid| pid.downcast::<NSNumber>().ok())
                .map(|pid| pid.intValue() as u32);

            if active_app_pid == self.pid().ok() {
                return Ok(true);
            }

            Ok(false)
        }
    }

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        let window_cf_dictionary = get_window_cf_dictionary(self.window_id)?;

        let cg_rect = get_window_cg_rect(window_cf_dictionary.as_ref())?;

        capture(
            cg_rect,
            CGWindowListOption::OptionIncludingWindow,
            self.window_id,
        )
    }
}
