use core::slice;
use std::{cmp::Ordering, ffi::c_void, mem, ptr};

use image::RgbaImage;
use widestring::U16CString;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{BOOL, HANDLE, HWND, LPARAM, MAX_PATH, RECT, TRUE},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::{IsRectEmpty, MonitorFromWindow, MONITOR_DEFAULTTONEAREST},
        },
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            ProcessStatus::{GetModuleBaseNameW, GetModuleFileNameExW},
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowInfo, GetWindowLongPtrW,
            GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
            IsWindowVisible, IsZoomed, GWL_EXSTYLE, WINDOWINFO, WINDOW_EX_STYLE, WS_EX_TOOLWINDOW,
        },
    },
};

use crate::{error::XCapResult, platform::utils::log_last_error};

use super::{
    capture::capture_window,
    impl_monitor::ImplMonitor,
    utils::{get_process_is_dpi_awareness, open_process},
};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub hwnd: HWND,
    #[allow(unused)]
    pub window_info: WINDOWINFO,
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
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() {
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

        let class_name = U16CString::from_vec_truncate(&lp_class_name[0..lp_class_name_length])
            .to_string()
            .unwrap_or_default();
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
        let lp_dw_process_id = get_window_pid(hwnd);
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

        let mut rect = RECT::default();

        let get_rect_is_err = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            mem::size_of::<RECT>() as u32,
        )
        .is_err();

        if get_rect_is_err {
            return false;
        }

        if IsRectEmpty(&rect).as_bool() {
            return false;
        }
    }

    true
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, state: LPARAM) -> BOOL {
    let state = Box::leak(Box::from_raw(state.0 as *mut (Vec<(HWND, i32)>, i32)));

    if is_valid_window(hwnd) {
        state.0.push((hwnd, state.1));
    }

    state.1 += 1;

    TRUE
}

fn get_window_title(hwnd: HWND) -> XCapResult<String> {
    unsafe {
        let text_length = GetWindowTextLengthW(hwnd);
        let mut wide_buffer = vec![0u16; (text_length + 1) as usize];
        GetWindowTextW(hwnd, &mut wide_buffer);
        let window_title = U16CString::from_vec_truncate(wide_buffer).to_string()?;

        Ok(window_title)
    }
}

#[derive(Debug, Default)]
struct LangCodePage {
    pub w_language: u16,
    pub w_code_page: u16,
}

fn get_module_basename(handle: HANDLE) -> XCapResult<String> {
    unsafe {
        // 默认使用 module_basename
        let mut module_base_name_w = [0; MAX_PATH as usize];
        let result = GetModuleBaseNameW(handle, None, &mut module_base_name_w);

        if result == 0 {
            log_last_error("GetModuleBaseNameW");

            GetModuleFileNameExW(Some(handle), None, &mut module_base_name_w);
        }

        let module_basename = U16CString::from_vec_truncate(module_base_name_w).to_string()?;

        Ok(module_basename)
    }
}

fn get_window_pid(hwnd: HWND) -> u32 {
    unsafe {
        let mut lp_dw_process_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut lp_dw_process_id));
        lp_dw_process_id
    }
}

fn get_app_name(pid: u32) -> XCapResult<String> {
    unsafe {
        let scope_guard_handle = match open_process(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(box_handle) => box_handle,
            Err(err) => {
                log::error!("{}", err);
                return Ok(String::new());
            }
        };

        let mut filename = [0; MAX_PATH as usize];
        GetModuleFileNameExW(Some(*scope_guard_handle), None, &mut filename);

        let pcw_filename = PCWSTR::from_raw(filename.as_ptr());

        let file_version_info_size_w = GetFileVersionInfoSizeW(pcw_filename, None);
        if file_version_info_size_w == 0 {
            log_last_error("GetFileVersionInfoSizeW");

            return get_module_basename(*scope_guard_handle);
        }

        let mut file_version_info = vec![0u16; file_version_info_size_w as usize];

        GetFileVersionInfoW(
            pcw_filename,
            None,
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
                let attr = U16CString::from_vec_truncate(value).to_string()?;
                let attr = attr.trim();

                if !attr.is_empty() {
                    return Ok(attr.to_string());
                }
            }
        }

        get_module_basename(*scope_guard_handle)
    }
}

impl ImplWindow {
    fn new(hwnd: HWND, z: i32) -> XCapResult<ImplWindow> {
        unsafe {
            let mut window_info = WINDOWINFO {
                cbSize: mem::size_of::<WINDOWINFO>() as u32,
                ..WINDOWINFO::default()
            };

            GetWindowInfo(hwnd, &mut window_info)?;

            let title = get_window_title(hwnd)?;
            let pid = get_window_pid(hwnd);
            let app_name = get_app_name(pid)?;

            let h_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let rc_client = window_info.rcClient;
            let is_minimized = IsIconic(hwnd).as_bool();
            let is_maximized = IsZoomed(hwnd).as_bool();

            Ok(ImplWindow {
                hwnd,
                window_info,
                id: hwnd.0 as u32,
                title,
                app_name,
                pid,
                current_monitor: ImplMonitor::new(h_monitor)?,
                x: rc_client.left,
                y: rc_client.top,
                z,
                width: (rc_client.right - rc_client.left) as u32,
                height: (rc_client.bottom - rc_client.top) as u32,
                is_minimized,
                is_maximized,
            })
        }
    }

    pub fn is_focused(&self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        // (HWND, i32) 表示当前窗口以及层级，既（窗口，层级 z），i32 表示 max_z_order，既最大的窗口的 z 顺序
        // 窗口当前层级为 max_z_order - z
        let hwnds_mut_ptr: *mut (Vec<(HWND, i32)>, i32) = Box::into_raw(Box::default());

        let hwnds = unsafe {
            // EnumWindows 函数按照 Z 顺序遍历顶层窗口，从最顶层的窗口开始，依次向下遍历。
            EnumWindows(Some(enum_windows_proc), LPARAM(hwnds_mut_ptr as isize))?;
            Box::from_raw(hwnds_mut_ptr)
        };

        let mut impl_windows = Vec::new();

        let max_z_order = hwnds.1;

        for &(hwnd, z) in hwnds.0.iter() {
            if let Ok(impl_window) = ImplWindow::new(hwnd, max_z_order - z) {
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
        // 在win10之后，不同窗口有不同的dpi，所以可能存在截图不全或者截图有较大空白，实际窗口没有填充满图片
        // 如果窗口不感知dpi，那么就不需要缩放，如果当前进程感知dpi，那么也不需要缩放
        let scope_guard_handle = open_process(PROCESS_QUERY_LIMITED_INFORMATION, false, self.pid)?;
        let window_is_dpi_awareness = get_process_is_dpi_awareness(*scope_guard_handle)?;
        let current_process_is_dpi_awareness =
            unsafe { get_process_is_dpi_awareness(GetCurrentProcess())? };

        let scale_factor = if !window_is_dpi_awareness {
            1.0
        } else if current_process_is_dpi_awareness {
            1.0
        } else {
            self.current_monitor.scale_factor
        };

        capture_window(self.hwnd, scale_factor, &self.window_info)
    }
}
