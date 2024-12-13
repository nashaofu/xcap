use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetValueAtIndex},
    base::{FromVoid, TCFType},
    dictionary::{CFDictionaryGetValue, CFDictionaryRef},
    number::{kCFNumberIntType, CFBooleanGetValue, CFBooleanRef, CFNumberGetValue, CFNumberRef},
    string::CFString,
};
use core_graphics::{
    display::{
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
        kCGWindowListOptionOnScreenOnly, CGDisplay, CGPoint, CGSize, CGWindowListCopyWindowInfo,
    },
    geometry::CGRect,
    window::{kCGNullWindowID, kCGWindowSharingNone},
};
use image::RgbaImage;
use std::ffi::c_void;

use crate::{error::XCapResult, XCapError};

use super::{boxed::BoxCFArrayRef, capture::capture, impl_monitor::ImplMonitor};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
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
    pub is_focused: bool,
}

unsafe impl Send for ImplWindow {}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGRectMakeWithDictionaryRepresentation(
        dict: CFDictionaryRef,
        rect: &mut CGRect,
    ) -> CFBooleanRef;
    fn CGEventSourceCreate(stateID: i32) -> *mut c_void;
    fn CGEventGetUnflippedLocation(event: *mut c_void) -> CGPoint;
    fn CGEventCreate(source: *mut c_void) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
}

fn get_cf_dictionary_get_value(
    cf_dictionary_ref: CFDictionaryRef,
    key: &str,
) -> XCapResult<*const c_void> {
    unsafe {
        let cf_dictionary_key = CFString::new(key);

        let value = CFDictionaryGetValue(cf_dictionary_ref, cf_dictionary_key.as_CFTypeRef());

        if value.is_null() {
            return Err(XCapError::new(format!(
                "Get CFDictionary {} value failed",
                key
            )));
        }

        Ok(value)
    }
}

fn get_cf_number_u32_value(cf_dictionary_ref: CFDictionaryRef, key: &str) -> XCapResult<u32> {
    unsafe {
        let cf_number_ref = get_cf_dictionary_get_value(cf_dictionary_ref, key)?;

        let mut value: u32 = 0;
        let is_success = CFNumberGetValue(
            cf_number_ref as CFNumberRef,
            kCFNumberIntType,
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

fn get_cf_string_value(cf_dictionary_ref: CFDictionaryRef, key: &str) -> XCapResult<String> {
    let value_ref = get_cf_dictionary_get_value(cf_dictionary_ref, key)?;

    Ok(unsafe { CFString::from_void(value_ref).to_string() })
}

fn get_cf_bool_value(cf_dictionary_ref: CFDictionaryRef, key: &str) -> XCapResult<bool> {
    let value_ref = get_cf_dictionary_get_value(cf_dictionary_ref, key)?;

    Ok(unsafe { CFBooleanGetValue(value_ref as CFBooleanRef) })
}

fn get_cf_number_i32_value(cf_dictionary_ref: CFDictionaryRef, key: &str) -> XCapResult<i32> {
    unsafe {
        let cf_number_ref = get_cf_dictionary_get_value(cf_dictionary_ref, key)?;

        let mut value: i32 = 0;
        let is_success = CFNumberGetValue(
            cf_number_ref as CFNumberRef,
            kCFNumberIntType,
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

fn get_window_cg_rect(window_cf_dictionary_ref: CFDictionaryRef) -> XCapResult<CGRect> {
    unsafe {
        let window_bounds_ref =
            get_cf_dictionary_get_value(window_cf_dictionary_ref, "kCGWindowBounds")?
                as CFDictionaryRef;

        let mut cg_rect = CGRect::default();

        let is_success_ref =
            CGRectMakeWithDictionaryRepresentation(window_bounds_ref, &mut cg_rect);

        if is_success_ref.is_null() {
            return Err(XCapError::new(
                "CGRectMakeWithDictionaryRepresentation failed",
            ));
        }

        Ok(cg_rect)
    }
}

fn get_mouse_position() -> CGPoint {
    unsafe {
        // HIDSystemState = 1
        let source = CGEventSourceCreate(1);
        let event = CGEventCreate(source);
        let position = CGEventGetUnflippedLocation(event);
        CFRelease(event);
        CFRelease(source);
        position
    }
}

impl ImplWindow {
    pub fn new(
        window_cf_dictionary_ref: CFDictionaryRef,
        impl_monitors: &[ImplMonitor],
        window_name: String,
        window_owner_name: String,
    ) -> XCapResult<ImplWindow> {
        let id = get_cf_number_u32_value(window_cf_dictionary_ref, "kCGWindowNumber")?;
        let cg_rect = get_window_cg_rect(window_cf_dictionary_ref)?;

        let primary_monitor = ImplMonitor::new(CGDisplay::main().id)?;

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
                .unwrap_or(&primary_monitor);

            (
                cg_rect.size.width as u32 >= impl_monitor.width
                    && cg_rect.size.height as u32 >= impl_monitor.height,
                impl_monitor,
            )
        };

        let is_minimized =
            !get_cf_bool_value(window_cf_dictionary_ref, "kCGWindowIsOnscreen")? && !is_maximized;

        let is_focused = {
            let mouse_pos = get_mouse_position();
            cg_rect.contains(&mouse_pos)
        };

        Ok(ImplWindow {
            id,
            title: window_name,
            app_name: window_owner_name,
            current_monitor: current_monitor.clone(),
            x: cg_rect.origin.x as i32,
            y: cg_rect.origin.y as i32,
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
            let mut impl_windows = Vec::new();

            let box_cf_array_ref = BoxCFArrayRef::new(CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            ));

            if box_cf_array_ref.is_null() {
                return Ok(impl_windows);
            }

            let num_windows = CFArrayGetCount(*box_cf_array_ref);

            for i in 0..num_windows {
                let window_cf_dictionary_ref =
                    CFArrayGetValueAtIndex(*box_cf_array_ref, i) as CFDictionaryRef;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let window_name =
                    match get_cf_string_value(window_cf_dictionary_ref, "kCGWindowName") {
                        Ok(window_name) => window_name,
                        _ => continue,
                    };

                let window_owner_name =
                    match get_cf_string_value(window_cf_dictionary_ref, "kCGWindowOwnerName") {
                        Ok(window_owner_name) => window_owner_name,
                        _ => continue,
                    };

                if window_name.eq("StatusIndicator") && window_owner_name.eq("Window Server") {
                    continue;
                }

                let window_sharing_state = match get_cf_number_u32_value(
                    window_cf_dictionary_ref,
                    "kCGWindowSharingState",
                ) {
                    Ok(window_sharing_state) => window_sharing_state,
                    _ => continue,
                };

                if window_sharing_state == kCGWindowSharingNone {
                    continue;
                }

                if let Ok(impl_window) = ImplWindow::new(
                    window_cf_dictionary_ref,
                    &impl_monitors,
                    window_name.clone(),
                    window_owner_name.clone(),
                ) {
                    impl_windows.push(impl_window);
                } else {
                    log::error!(
                        "ImplWindow::new({:?}, {:?}, {:?}, {:?}) failed",
                        window_cf_dictionary_ref,
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
    pub fn refresh(&mut self) -> XCapResult<()> {
        unsafe {
            let impl_monitors = ImplMonitor::all()?;

            let box_cf_array_ref = BoxCFArrayRef::new(CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            ));

            if box_cf_array_ref.is_null() {
                return Err(XCapError::new("Run CGWindowListCopyWindowInfo error"));
            }

            let num_windows = CFArrayGetCount(*box_cf_array_ref);

            for i in 0..num_windows {
                let window_cf_dictionary_ref =
                    CFArrayGetValueAtIndex(*box_cf_array_ref, i) as CFDictionaryRef;

                if window_cf_dictionary_ref.is_null() {
                    continue;
                }

                let k_cg_window_number =
                    get_cf_number_u32_value(window_cf_dictionary_ref, "kCGWindowNumber")?;

                if k_cg_window_number == self.id {
                    let window_name =
                        match get_cf_string_value(window_cf_dictionary_ref, "kCGWindowName") {
                            Ok(window_name) => window_name,
                            _ => return Err(XCapError::new("Get window name failed")),
                        };

                    let window_owner_name =
                        match get_cf_string_value(window_cf_dictionary_ref, "kCGWindowOwnerName") {
                            Ok(window_owner_name) => window_owner_name,
                            _ => return Err(XCapError::new("Get window owner name failed")),
                        };

                    let impl_window = ImplWindow::new(
                        window_cf_dictionary_ref,
                        &impl_monitors,
                        window_name,
                        window_owner_name,
                    )?;

                    self.id = impl_window.id;
                    self.title = impl_window.title;
                    self.app_name = impl_window.app_name;
                    self.current_monitor = impl_window.current_monitor;
                    self.x = impl_window.x;
                    self.y = impl_window.y;
                    self.width = impl_window.width;
                    self.height = impl_window.height;
                    self.is_minimized = impl_window.is_minimized;
                    self.is_maximized = impl_window.is_maximized;
                    self.is_focused = impl_window.is_focused;

                    return Ok(());
                }
            }

            Err(XCapError::new("Not Found window"))
        }
    }
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture(
            CGRect::new(
                &CGPoint::new(self.x as f64, self.y as f64),
                &CGSize::new(self.width as f64, self.height as f64),
            ),
            kCGWindowListOptionIncludingWindow,
            self.id,
        )
    }
}
