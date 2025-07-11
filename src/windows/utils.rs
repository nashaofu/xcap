use std::mem;

use image::RgbaImage;
use scopeguard::{ScopeGuard, guard};
use widestring::U16CString;
use windows::{
    Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
            QueryDisplayConfig,
        },
        Foundation::{CloseHandle, FreeLibrary, GetLastError, HANDLE, HMODULE, HWND},
        Graphics::Gdi::MONITORINFOEXW,
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
            Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS},
        },
        UI::WindowsAndMessaging::{GetWindowInfo, WINDOWINFO},
    },
    core::{HRESULT, PCWSTR, s, w},
};

use crate::{XCapError, error::XCapResult};

pub(super) fn get_build_number() -> u32 {
    unsafe {
        let mut buf_len: u32 = 2048;
        let mut buf: Vec<u16> = Vec::with_capacity(buf_len as usize);

        let err = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut buf_len),
        );

        if err.is_err() {
            return 0;
        }

        buf.set_len(buf_len as usize);

        let build_version = U16CString::from_vec_truncate(buf)
            .to_string()
            .unwrap_or_default();

        build_version.parse().unwrap_or(0)
    }
}

pub(super) fn get_os_major_version() -> u8 {
    let build_number = get_build_number();
    // https://en.wikipedia.org/wiki/List_of_Microsoft_Windows_versions
    if build_number >= 22000 {
        11
    } else if build_number >= 10240 {
        10
    } else if build_number >= 9200 {
        8
    } else {
        7
    }
}

pub(super) fn bgra_to_rgba(mut buffer: Vec<u8>) -> Vec<u8> {
    let is_old_version = get_os_major_version() < 8;
    for src in buffer.chunks_exact_mut(4) {
        src.swap(0, 2);
        // fix https://github.com/nashaofu/xcap/issues/92#issuecomment-1910014951
        if src[3] == 0 && is_old_version {
            src[3] = 255;
        }
    }

    buffer
}

pub(super) fn bgra_to_rgba_image(
    width: u32,
    height: u32,
    buffer: Vec<u8>,
) -> XCapResult<RgbaImage> {
    RgbaImage::from_raw(width, height, bgra_to_rgba(buffer))
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
}

// 定义 GetProcessDpiAwareness 函数的类型
type GetProcessDpiAwareness =
    unsafe extern "system" fn(hprocess: HANDLE, value: *mut u32) -> HRESULT;

pub(super) fn get_process_is_dpi_awareness(process: HANDLE) -> XCapResult<bool> {
    unsafe {
        let scope_guard_hmodule = load_library(w!("Shcore.dll"))?;

        let get_process_dpi_awareness_proc_address =
            GetProcAddress(*scope_guard_hmodule, s!("GetProcessDpiAwareness")).ok_or(
                XCapError::new("GetProcAddress GetProcessDpiAwareness failed"),
            )?;

        let get_process_dpi_awareness: GetProcessDpiAwareness =
            mem::transmute(get_process_dpi_awareness_proc_address);

        let mut process_dpi_awareness = 0;
        // https://learn.microsoft.com/zh-cn/windows/win32/api/shellscalingapi/nf-shellscalingapi-getprocessdpiawareness
        get_process_dpi_awareness(process, &mut process_dpi_awareness).ok()?;

        // 当前进程不感知 DPI，则回退到 GetDeviceCaps 获取 DPI
        Ok(process_dpi_awareness != 0)
    }
}

pub(super) fn load_library(
    lib_filename: PCWSTR,
) -> XCapResult<ScopeGuard<HMODULE, impl FnOnce(HMODULE)>> {
    unsafe {
        let hmodule = LoadLibraryW(lib_filename)?;

        if hmodule.is_invalid() {
            return Err(XCapError::new(format!(
                "LoadLibraryW error {:?}",
                GetLastError()
            )));
        }

        let scope_guard_hmodule = guard(hmodule, |val| {
            if let Err(err) = FreeLibrary(val) {
                log::error!("FreeLibrary {val:?} failed {err:?}");
            }
        });

        Ok(scope_guard_hmodule)
    }
}

pub(super) fn open_process(
    dw_desired_access: PROCESS_ACCESS_RIGHTS,
    b_inherit_handle: bool,
    dw_process_id: u32,
) -> XCapResult<ScopeGuard<HANDLE, impl FnOnce(HANDLE)>> {
    unsafe {
        let handle = OpenProcess(dw_desired_access, b_inherit_handle, dw_process_id)?;

        if handle.is_invalid() {
            return Err(XCapError::new(format!(
                "OpenProcess error {:?}",
                GetLastError()
            )));
        }

        let scope_guard_handle = guard(handle, |val| {
            if let Err(err) = CloseHandle(val) {
                log::error!("CloseHandle {val:?} failed {err:?}");
            }
        });

        Ok(scope_guard_handle)
    }
}

pub(super) fn get_monitor_config(
    monitor_info_ex_w: MONITORINFOEXW,
) -> XCapResult<DISPLAYCONFIG_TARGET_DEVICE_NAME> {
    unsafe {
        let mut number_of_paths = 0;
        let mut number_of_modes = 0;
        GetDisplayConfigBufferSizes(
            QDC_ONLY_ACTIVE_PATHS,
            &mut number_of_paths,
            &mut number_of_modes,
        )
        .ok()?;

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); number_of_paths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); number_of_modes as usize];

        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut number_of_paths,
            paths.as_mut_ptr(),
            &mut number_of_modes,
            modes.as_mut_ptr(),
            None,
        )
        .ok()?;

        for path in paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..DISPLAYCONFIG_SOURCE_DEVICE_NAME::default()
            };

            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }

            if source.viewGdiDeviceName != monitor_info_ex_w.szDevice {
                continue;
            }

            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    size: mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.targetInfo.id,
                },
                ..DISPLAYCONFIG_TARGET_DEVICE_NAME::default()
            };

            if DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }

            return Ok(target);
        }

        Err(XCapError::new("Get monitor name failed"))
    }
}

pub fn get_window_info(hwnd: HWND) -> XCapResult<WINDOWINFO> {
    let mut window_info = WINDOWINFO {
        cbSize: mem::size_of::<WINDOWINFO>() as u32,
        ..WINDOWINFO::default()
    };

    unsafe {
        GetWindowInfo(hwnd, &mut window_info)?;
    };

    Ok(window_info)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW,
        MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    use super::*;

    /// 特别说明
    /// 以下测试均以 windows11 作为测试环境
    #[test]
    fn test_get_build_number() {
        let build = get_build_number();
        println!("build {build}");
        assert!(build == 26100, "build number should be 26100");
    }

    #[test]
    fn test_get_os_major_version() {
        let version = get_os_major_version();
        assert!(version == 11, "os major version should be 11");
    }

    #[test]
    fn test_bgra_to_rgba() {
        let input = vec![0, 1, 2, 255, 4, 5, 6, 255];
        let output = bgra_to_rgba(input);
        assert_eq!(output, vec![2, 1, 0, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn test_bgra_to_rgba_image() {
        let width = 2;
        let height = 1;
        let buffer = vec![0, 1, 2, 255, 4, 5, 6, 255];
        let result = bgra_to_rgba_image(width, height, buffer);

        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width(), width);
        assert_eq!(image.height(), height);
    }

    #[test]
    fn test_get_process_is_dpi_awareness() {
        // // Modify the program's DPI awareness. You can set the value to PROCESS_DPI_UNAWARE or PROCESS_PER_MONITOR_DPI_AWARE for testing.
        // SetProcessDpiAwareness(PROCESS_DPI_UNAWARE).unwrap();

        // let process = GetCurrentProcess();
        // let is_dpi_awareness = get_process_is_dpi_awareness(process).unwrap();

        // assert!(!is_dpi_awareness);
    }
    #[test]
    fn test_load_library() {
        let scope_guard_hmodule = load_library(w!("Shcore.dll")).unwrap();

        assert!(!scope_guard_hmodule.is_invalid());
    }

    #[test]
    fn test_open_process() {
        unsafe {
            let process_id = GetCurrentProcessId();
            let scope_guard_handle =
                open_process(PROCESS_QUERY_LIMITED_INFORMATION, true, process_id).unwrap();

            assert!(!scope_guard_handle.is_invalid());
        }
    }

    fn get_monitor_info_ex_w() -> MONITORINFOEXW {
        unsafe {
            let point = POINT { x: 0, y: 0 };
            let h_monitor = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
            let mut monitor_info_ex_w = MONITORINFOEXW::default();
            monitor_info_ex_w.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
            let monitor_info_ex_w_ptr =
                &mut monitor_info_ex_w as *mut MONITORINFOEXW as *mut MONITORINFO;

            // https://learn.microsoft.com/zh-cn/windows/win32/api/winuser/nf-winuser-getmonitorinfoa
            GetMonitorInfoW(h_monitor, monitor_info_ex_w_ptr)
                .ok()
                .unwrap();

            monitor_info_ex_w
        }
    }

    #[test]
    fn test_get_monitor_config() {
        let monitor_info_ex_w = get_monitor_info_ex_w();
        // 获取显示器配置信息
        let monitor_config = get_monitor_config(monitor_info_ex_w).unwrap();

        assert!(!monitor_config.monitorFriendlyDeviceName.is_empty());
    }

    #[test]
    fn test_get_window_info() {
        unsafe {
            let monitor_info_ex_w = get_monitor_info_ex_w();
            let mut dev_mode_w = DEVMODEW {
                dmSize: mem::size_of::<DEVMODEW>() as u16,
                ..DEVMODEW::default()
            };
            EnumDisplaySettingsW(
                PCWSTR(monitor_info_ex_w.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut dev_mode_w,
            )
            .ok()
            .unwrap();

            let hwnd = GetDesktopWindow();
            let window_info = get_window_info(hwnd).unwrap();
            let width = (window_info.rcWindow.right - window_info.rcWindow.left) as u32;

            assert!(width == dev_mode_w.dmPelsWidth);
        }
    }
}
