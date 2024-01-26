use image::RgbaImage;
use std::{ffi::c_void, mem};
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, TRUE},
    Graphics::{
        Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWM_CLOAKED_SHELL},
        Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST},
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetLastActivePopup, GetWindowInfo, GetWindowLongW,
        GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible, IsZoomed, GA_ROOTOWNER,
        GWL_EXSTYLE, WINDOWINFO, WINDOW_EX_STYLE, WS_EX_TOOLWINDOW,
    },
};

use crate::error::XCapResult;

use super::{capture::capture_window, impl_monitor::ImplMonitor, utils::wide_string_to_string};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub hwnd: HWND,
    #[allow(unused)]
    pub window_info: WINDOWINFO,
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

fn is_valid_window(hwnd: HWND) -> bool {
    unsafe {
        // ignore invisible windows
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        // ignore windows in other virtual desktops
        let mut cloaked = 0u32;

        let is_dwm_get_window_attribute_fail = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            mem::size_of::<u32>() as u32,
        )
        .is_err();

        if is_dwm_get_window_attribute_fail {
            return false;
        }

        // windows in other virtual desktops have the DWM_CLOAKED_SHELL bit set
        if cloaked & DWM_CLOAKED_SHELL != 0 {
            return false;
        }

        // https://stackoverflow.com/questions/7277366
        let mut hwnd_walk = None;

        // Start at the root owner
        let mut hwnd_tray = GetAncestor(hwnd, GA_ROOTOWNER);

        // See if we are the last active visible popup
        while Some(hwnd_tray) != hwnd_walk {
            hwnd_walk = Some(hwnd_tray);
            hwnd_tray = GetLastActivePopup(hwnd_tray);

            if IsWindowVisible(hwnd_tray).as_bool() {
                break;
            }
        }

        if hwnd_walk != Some(hwnd) {
            return false;
        }

        // Tool windows should not be displayed either, these do not appear in the task bar.
        let window_ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

        if WINDOW_EX_STYLE(window_ex_style).contains(WS_EX_TOOLWINDOW) {
            return false;
        }
    }

    true
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, state: LPARAM) -> BOOL {
    if !is_valid_window(hwnd) {
        return TRUE;
    }

    let state = Box::leak(Box::from_raw(state.0 as *mut Vec<HWND>));
    state.push(hwnd);

    TRUE
}

impl ImplWindow {
    fn new(hwnd: HWND) -> XCapResult<ImplWindow> {
        unsafe {
            let mut window_info = WINDOWINFO::default();
            window_info.cbSize = mem::size_of::<WINDOWINFO>() as u32;

            GetWindowInfo(hwnd, &mut window_info)?;

            let title = {
                let text_length = GetWindowTextLengthW(hwnd);
                let mut wide_buffer = vec![0u16; (text_length + 1) as usize];
                GetWindowTextW(hwnd, &mut wide_buffer);
                wide_string_to_string(&wide_buffer)?
            };

            let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let rc_client = window_info.rcClient;
            let is_minimized = IsIconic(hwnd).as_bool();
            let is_maximized = IsZoomed(hwnd).as_bool();

            Ok(ImplWindow {
                hwnd,
                window_info,
                id: hwnd.0 as u32,
                title,
                app_name: String::from("Unsupported"),
                current_monitor: ImplMonitor::new(hmonitor)?,
                x: rc_client.left,
                y: rc_client.top,
                width: (rc_client.right - rc_client.left) as u32,
                height: (rc_client.bottom - rc_client.top) as u32,
                is_minimized,
                is_maximized,
            })
        }
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        let hwnds_mut_ptr: *mut Vec<HWND> = Box::into_raw(Box::default());

        let hwnds = unsafe {
            EnumWindows(Some(enum_windows_proc), LPARAM(hwnds_mut_ptr as isize))?;
            Box::from_raw(hwnds_mut_ptr)
        };

        let mut impl_windows = Vec::new();

        for &hwnd in hwnds.iter() {
            impl_windows.push(ImplWindow::new(hwnd)?);
        }

        Ok(impl_windows)
    }
}

impl ImplWindow {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        let width = ((self.width as f32) * self.current_monitor.scale_factor) as i32;
        let height = ((self.height as f32) * self.current_monitor.scale_factor) as i32;

        capture_window(self.hwnd, width, height)
    }
}
