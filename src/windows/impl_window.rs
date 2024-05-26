use core::slice;
use image::RgbaImage;
use std::{cmp::Ordering, ffi::c_void, mem, ptr};
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{BOOL, HWND, LPARAM, MAX_PATH, TRUE},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
            Gdi::{IsRectEmpty, MonitorFromWindow, MONITOR_DEFAULTTONEAREST},
        },
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            ProcessStatus::{GetModuleBaseNameW, GetModuleFileNameExW},
            Threading::{GetCurrentProcessId, PROCESS_ALL_ACCESS},
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowInfo, GetWindowLongPtrW, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
            IsZoomed, GWL_EXSTYLE, WINDOWINFO, WINDOW_EX_STYLE, WS_EX_TOOLWINDOW,
        },
    },
};

use crate::{
    error::XCapResult,
    platform::{boxed::BoxProcessHandle, utils::log_last_error},
};

use super::{
    capture::capture_window,
    impl_monitor::ImplMonitor,
    utils::{get_window_rect, wide_string_to_string},
};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub hwnd: HWND,
    #[allow(unused)]
    pub window_info: WINDOWINFO,
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub process_id: u32,
    pub current_monitor: ImplMonitor,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_maximized: bool,
}

fn is_window_cloaked(hwnd: HWND) -> bool {
    unsafe {
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

        cloaked != 0
    }
}

// https://webrtc.googlesource.com/src.git/+/refs/heads/main/modules/desktop_capture/win/window_capture_utils.cc#52
fn is_valid_window(hwnd: HWND) -> bool {
    unsafe {
        // ignore invisible windows
        if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        // 特别说明，与webrtc中源码有区别，子窗口也枚举进来，所以就不需要下面的代码了：
        // HWND owner = GetWindow(hwnd, GW_OWNER);
        // LONG exstyle = GetWindowLong(hwnd, GWL_EXSTYLE);
        // if (owner && !(exstyle & WS_EX_APPWINDOW)) {
        //   return TRUE;
        // }

        let mut lp_class_name = [0u16; MAX_PATH as usize];
        let lp_class_name_length = GetClassNameW(hwnd, &mut lp_class_name) as usize;
        if lp_class_name_length < 1 {
            return false;
        }

        let class_name =
            wide_string_to_string(&lp_class_name[0..lp_class_name_length]).unwrap_or_default();
        if class_name.is_empty() {
            return false;
        }

        let gwl_ex_style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let title = get_window_title(hwnd).unwrap_or_default();

        // 过滤掉具有 WS_EX_TOOLWINDOW 样式的窗口
        if gwl_ex_style.contains(WS_EX_TOOLWINDOW) {
            // windows 任务栏可以捕获
            if class_name.cmp(&String::from("Shell_TrayWnd")) != Ordering::Equal && title.is_empty()
            {
                return false;
            }
        }

        // GetWindowText* are potentially blocking operations if `hwnd` is
        // owned by the current process. The APIs will send messages to the window's
        // message loop, and if the message loop is waiting on this operation we will
        // enter a deadlock.
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowtexta#remarks
        //
        // To help consumers avoid this, there is a DesktopCaptureOption to ignore
        // windows owned by the current process. Consumers should either ensure that
        // the thread running their message loop never waits on this operation, or use
        // the option to exclude these windows from the source list.
        let lp_dw_process_id = get_process_id(hwnd);
        if lp_dw_process_id == GetCurrentProcessId() {
            return false;
        }

        // Skip Program Manager window.
        if class_name.cmp(&String::from("Progman")) == Ordering::Equal {
            return false;
        }
        // Skip Start button window on Windows Vista, Windows 7.
        // On Windows 8, Windows 8.1, Windows 10 Start button is not a top level
        // window, so it will not be examined here.
        if class_name.cmp(&String::from("Button")) == Ordering::Equal {
            return false;
        }

        if is_window_cloaked(hwnd) {
            return false;
        }

        let is_rect_empty = get_window_rect(hwnd).is_ok_and(|rect| IsRectEmpty(&rect).as_bool());

        if is_rect_empty {
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

fn get_window_title(hwnd: HWND) -> XCapResult<String> {
    unsafe {
        let text_length = GetWindowTextLengthW(hwnd);
        let mut wide_buffer = vec![0u16; (text_length + 1) as usize];
        GetWindowTextW(hwnd, &mut wide_buffer);
        wide_string_to_string(&wide_buffer)
    }
}

#[derive(Debug, Default)]
struct LangCodePage {
    pub w_language: u16,
    pub w_code_page: u16,
}

fn get_module_basename(box_process_handle: BoxProcessHandle) -> XCapResult<String> {
    unsafe {
        // 默认使用 module_basename
        let mut module_base_name_w = [0; MAX_PATH as usize];
        let result = GetModuleBaseNameW(*box_process_handle, None, &mut module_base_name_w);

        if result == 0 {
            log_last_error("GetModuleBaseNameW");

            GetModuleFileNameExW(*box_process_handle, None, &mut module_base_name_w);
        }

        wide_string_to_string(&module_base_name_w)
    }
}

fn get_process_id(hwnd: HWND) -> u32 {
    unsafe {
        let mut lp_dw_process_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut lp_dw_process_id));
        lp_dw_process_id
    }
}

fn get_app_name(hwnd: HWND) -> XCapResult<String> {
    unsafe {
        let lp_dw_process_id = get_process_id(hwnd);

        let box_process_handle =
            match BoxProcessHandle::open(PROCESS_ALL_ACCESS, false, lp_dw_process_id) {
                Ok(box_handle) => box_handle,
                Err(err) => {
                    log::error!("{}", err);
                    return Ok(String::new());
                }
            };

        let mut filename = [0; MAX_PATH as usize];
        GetModuleFileNameExW(*box_process_handle, None, &mut filename);

        let pcw_filename = PCWSTR::from_raw(filename.as_ptr());

        let file_version_info_size_w = GetFileVersionInfoSizeW(pcw_filename, None);
        if file_version_info_size_w == 0 {
            log_last_error("GetFileVersionInfoSizeW");

            return get_module_basename(box_process_handle);
        }

        let mut file_version_info = vec![0u16; file_version_info_size_w as usize];

        GetFileVersionInfoW(
            pcw_filename,
            0,
            file_version_info_size_w,
            file_version_info.as_mut_ptr().cast(),
        )?;

        let mut lang_code_pages_ptr = ptr::null_mut();
        let mut lang_code_pages_length = 0;

        VerQueryValueW(
            file_version_info.as_ptr().cast(),
            &HSTRING::from("\\VarFileInfo\\Translation"),
            &mut lang_code_pages_ptr,
            &mut lang_code_pages_length,
        )
        .ok()?;

        let lang_code_pages: &[LangCodePage] =
            slice::from_raw_parts(lang_code_pages_ptr.cast(), lang_code_pages_length as usize);

        // 按照 keys 的顺序读取文件的属性值
        // 优先读取 FileDescription
        let keys = [
            "FileDescription",
            "ProductName",
            "ProductShortName",
            "InternalName",
            "OriginalFilename",
        ];

        for key in keys {
            for lang_code_page in lang_code_pages {
                let query_key = HSTRING::from(format!(
                    "\\StringFileInfo\\{:04x}{:04x}\\{}",
                    lang_code_page.w_language, lang_code_page.w_code_page, key
                ));

                let mut value_ptr = ptr::null_mut();
                let mut value_length: u32 = 0;

                let is_success = VerQueryValueW(
                    file_version_info.as_ptr().cast(),
                    &query_key,
                    &mut value_ptr,
                    &mut value_length,
                )
                .as_bool();

                if !is_success {
                    continue;
                }

                let value = slice::from_raw_parts(value_ptr.cast(), value_length as usize);
                let attr = wide_string_to_string(value)?;
                let attr = attr.trim();

                if !attr.is_empty() {
                    return Ok(attr.to_string());
                }
            }
        }

        get_module_basename(box_process_handle)
    }
}

impl ImplWindow {
    fn new(hwnd: HWND) -> XCapResult<ImplWindow> {
        unsafe {
            let mut window_info = WINDOWINFO {
                cbSize: mem::size_of::<WINDOWINFO>() as u32,
                ..WINDOWINFO::default()
            };

            GetWindowInfo(hwnd, &mut window_info)?;

            let title = get_window_title(hwnd)?;
            let app_name = get_app_name(hwnd)?;

            let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let rc_window = window_info.rcWindow;
            let is_minimized = IsIconic(hwnd).as_bool();
            let is_maximized = IsZoomed(hwnd).as_bool();

            Ok(ImplWindow {
                hwnd,
                window_info,
                id: hwnd.0 as u32,
                title,
                app_name,
                process_id: get_process_id(hwnd),
                current_monitor: ImplMonitor::new(hmonitor)?,
                x: rc_window.left,
                y: rc_window.top,
                width: (rc_window.right - rc_window.left) as u32,
                height: (rc_window.bottom - rc_window.top) as u32,
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
            if let Ok(impl_window) = ImplWindow::new(hwnd) {
                impl_windows.push(impl_window);
            } else {
                log::error!("ImplWindow::new({:?}) failed", hwnd);
            }
        }

        Ok(impl_windows)
    }
}

impl ImplWindow {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        // TODO: 在win10之后，不同窗口有不同的dpi，所以可能存在截图不全或者截图有较大空白，实际窗口没有填充满图片
        capture_window(self.hwnd, self.current_monitor.scale_factor)
    }
}
