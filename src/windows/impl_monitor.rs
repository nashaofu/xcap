use std::{mem, ptr, sync::mpsc::Receiver};

use image::RgbaImage;
use scopeguard::guard;
use widestring::U16CString;
use windows::{
    Win32::{
        Devices::Display::DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
        Foundation::{GetLastError, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::{
            CreateDCW, DESKTOPHORZRES, DEVMODEW, DMDO_90, DMDO_180, DMDO_270, DMDO_DEFAULT,
            DeleteDC, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
            GetDeviceCaps, GetMonitorInfoW, HDC, HMONITOR, HORZRES, MONITOR_DEFAULTTONULL,
            MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
        },
        System::{LibraryLoader::GetProcAddress, Threading::GetCurrentProcess},
        UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
    },
    core::{BOOL, HRESULT, PCWSTR, s, w},
};

use crate::{
    error::{XCapError, XCapResult},
    video_recorder::Frame,
};

use super::{
    capture::capture_monitor,
    impl_video_recorder::ImplVideoRecorder,
    utils::{get_monitor_config, get_process_is_dpi_awareness, load_library},
};

// A 函数与 W 函数区别
// https://learn.microsoft.com/zh-cn/windows/win32/learnwin32/working-with-strings

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    pub h_monitor: HMONITOR,
}

extern "system" fn monitor_enum_proc(
    h_monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    state: LPARAM,
) -> BOOL {
    unsafe {
        let state = Box::leak(Box::from_raw(state.0 as *mut Vec<HMONITOR>));
        state.push(h_monitor);

        TRUE
    }
}

fn get_monitor_info_ex_w(h_monitor: HMONITOR) -> XCapResult<MONITORINFOEXW> {
    let mut monitor_info_ex_w = MONITORINFOEXW::default();
    monitor_info_ex_w.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    let monitor_info_ex_w_ptr = &mut monitor_info_ex_w as *mut MONITORINFOEXW as *mut MONITORINFO;

    // https://learn.microsoft.com/zh-cn/windows/win32/api/winuser/nf-winuser-getmonitorinfoa
    unsafe { GetMonitorInfoW(h_monitor, monitor_info_ex_w_ptr).ok()? };

    Ok(monitor_info_ex_w)
}

fn get_dev_mode_w(h_monitor: HMONITOR) -> XCapResult<DEVMODEW> {
    let monitor_info_exw = get_monitor_info_ex_w(h_monitor)?;
    let sz_device = monitor_info_exw.szDevice.as_ptr();
    let mut dev_mode_w = DEVMODEW {
        dmSize: mem::size_of::<DEVMODEW>() as u16,
        ..DEVMODEW::default()
    };

    unsafe {
        EnumDisplaySettingsW(PCWSTR(sz_device), ENUM_CURRENT_SETTINGS, &mut dev_mode_w).ok()?;
    };

    Ok(dev_mode_w)
}

// 定义 GetDpiForMonitor 函数的类型
type GetDpiForMonitor = unsafe extern "system" fn(
    h_monitor: HMONITOR,
    dpi_type: u32,
    dpi_x: *mut u32,
    dpi_y: *mut u32,
) -> HRESULT;

fn get_hi_dpi_scale_factor(h_monitor: HMONITOR) -> XCapResult<f32> {
    unsafe {
        let current_process_is_dpi_awareness: bool =
            get_process_is_dpi_awareness(GetCurrentProcess())?;

        // 当前进程不感知 DPI，则回退到 GetDeviceCaps 获取 DPI
        if !current_process_is_dpi_awareness {
            return Err(XCapError::new("Process not DPI aware"));
        }

        let scope_guard_hmodule = load_library(w!("Shcore.dll"))?;

        let get_dpi_for_monitor_proc_address =
            GetProcAddress(*scope_guard_hmodule, s!("GetDpiForMonitor"))
                .ok_or(XCapError::new("GetProcAddress GetDpiForMonitor failed"))?;

        let get_dpi_for_monitor: GetDpiForMonitor =
            mem::transmute(get_dpi_for_monitor_proc_address);

        let mut dpi_x = 0;
        let mut dpi_y = 0;

        // https://learn.microsoft.com/zh-cn/windows/win32/api/shellscalingapi/ne-shellscalingapi-monitor_dpi_type
        get_dpi_for_monitor(h_monitor, 0, &mut dpi_x, &mut dpi_y).ok()?;

        Ok(dpi_x as f32 / 96.0)
    }
}

fn get_scale_factor(h_monitor: HMONITOR) -> XCapResult<f32> {
    let scale_factor = match get_hi_dpi_scale_factor(h_monitor) {
        Ok(val) => val,
        Err(err) => {
            log::info!("get_hi_dpi_scale_factor failed: {err}");
            let monitor_info_ex_w = get_monitor_info_ex_w(h_monitor)?;
            // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-getdevicecaps
            unsafe {
                let scope_guard_hdc = guard(
                    CreateDCW(
                        PCWSTR(monitor_info_ex_w.szDevice.as_ptr()),
                        PCWSTR(monitor_info_ex_w.szDevice.as_ptr()),
                        PCWSTR(ptr::null()),
                        None,
                    ),
                    |val| {
                        if !DeleteDC(val).as_bool() {
                            log::error!("DeleteDC({:?}) failed: {:?}", val, GetLastError());
                        }
                    },
                );

                let physical_width = GetDeviceCaps(Some(*scope_guard_hdc), DESKTOPHORZRES);
                let logical_width = GetDeviceCaps(Some(*scope_guard_hdc), HORZRES);

                physical_width as f32 / logical_width as f32
            }
        }
    };

    Ok(scale_factor)
}

impl ImplMonitor {
    pub fn new(h_monitor: HMONITOR) -> ImplMonitor {
        ImplMonitor { h_monitor }
    }

    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        let hmonitors_mut_ptr: *mut Vec<HMONITOR> = Box::into_raw(Box::default());

        let h_monitors = unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(monitor_enum_proc),
                LPARAM(hmonitors_mut_ptr as isize),
            )
            .ok()?;
            Box::from_raw(hmonitors_mut_ptr)
        };

        let mut impl_monitors = Vec::with_capacity(h_monitors.len());

        for &h_monitor in h_monitors.iter() {
            impl_monitors.push(ImplMonitor::new(h_monitor));
        }

        Ok(impl_monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<ImplMonitor> {
        let point = POINT { x, y };
        let h_monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };

        if h_monitor.is_invalid() {
            return Err(XCapError::new("Not found monitor"));
        }

        Ok(ImplMonitor::new(h_monitor))
    }
}

impl ImplMonitor {
    pub fn id(&self) -> XCapResult<u32> {
        Ok(self.h_monitor.0 as u32)
    }

    pub fn name(&self) -> XCapResult<String> {
        let monitor_info_ex_w = get_monitor_info_ex_w(self.h_monitor)?;

        let config_default_name = format!("Unknown Monitor {}", self.h_monitor.0 as u32);
        let config = match get_monitor_config(monitor_info_ex_w) {
            Ok(config) => config,
            Err(_) => return Ok(config_default_name),
        };

        let name = U16CString::from_vec_truncate(config.monitorFriendlyDeviceName).to_string()?;

        if name.is_empty() {
            return Ok(config_default_name);
        }

        Ok(name)
    }

    pub fn x(&self) -> XCapResult<i32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        let dm_position = unsafe { dev_mode_w.Anonymous1.Anonymous2.dmPosition };

        Ok(dm_position.x)
    }

    pub fn y(&self) -> XCapResult<i32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        let dm_position = unsafe { dev_mode_w.Anonymous1.Anonymous2.dmPosition };

        Ok(dm_position.y)
    }

    pub fn width(&self) -> XCapResult<u32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        Ok(dev_mode_w.dmPelsWidth)
    }

    pub fn height(&self) -> XCapResult<u32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        Ok(dev_mode_w.dmPelsHeight)
    }

    pub fn rotation(&self) -> XCapResult<f32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        let dm_display_orientation =
            unsafe { dev_mode_w.Anonymous1.Anonymous2.dmDisplayOrientation };
        let rotation = match dm_display_orientation {
            DMDO_90 => 90.0,
            DMDO_180 => 180.0,
            DMDO_270 => 270.0,
            DMDO_DEFAULT => 0.0,
            _ => 0.0,
        };
        Ok(rotation)
    }

    pub fn scale_factor(&self) -> XCapResult<f32> {
        get_scale_factor(self.h_monitor)
    }

    pub fn frequency(&self) -> XCapResult<f32> {
        let dev_mode_w = get_dev_mode_w(self.h_monitor)?;
        Ok(dev_mode_w.dmDisplayFrequency as f32)
    }

    pub fn is_primary(&self) -> XCapResult<bool> {
        let monitor_info_ex_w = get_monitor_info_ex_w(self.h_monitor)?;
        Ok(monitor_info_ex_w.monitorInfo.dwFlags == MONITORINFOF_PRIMARY)
    }

    pub fn is_builtin(&self) -> XCapResult<bool> {
        let monitor_info_ex_w = get_monitor_info_ex_w(self.h_monitor)?;
        let config = get_monitor_config(monitor_info_ex_w)?;

        Ok(config.outputTechnology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL)
    }

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        let x = self.x()?;
        let y = self.y()?;
        let width = self.width()?;
        let height = self.height()?;

        capture_monitor(x, y, width as i32, height as i32)
    }

    pub fn capture_region(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<RgbaImage> {
        // Validate region bounds
        let monitor_x = self.x()?;
        let monitor_y = self.y()?;
        let monitor_width = self.width()?;
        let monitor_height = self.height()?;

        if width > monitor_width
            || height > monitor_height
            || x + width > monitor_width
            || y + height > monitor_height
        {
            return Err(XCapError::InvalidCaptureRegion(format!(
                "Region ({x}, {y}, {width}, {height}) is outside monitor bounds ({monitor_x}, {monitor_y}, {monitor_width}, {monitor_height})"
            )));
        }

        // Calculate absolute coordinates
        let abs_x = monitor_x + x as i32;
        let abs_y = monitor_y + y as i32;

        capture_monitor(abs_x, abs_y, width as i32, height as i32)
    }

    pub fn video_recorder(&self) -> XCapResult<(ImplVideoRecorder, Receiver<Frame>)> {
        ImplVideoRecorder::new(self.h_monitor)
    }
}
