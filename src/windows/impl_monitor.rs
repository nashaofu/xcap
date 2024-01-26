use image::RgbaImage;
use std::mem;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{BOOL, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::{
            EnumDisplayMonitors, EnumDisplaySettingsW, GetDeviceCaps, GetMonitorInfoW,
            MonitorFromPoint, DESKTOPHORZRES, DEVMODEW, DMDO_180, DMDO_270, DMDO_90, DMDO_DEFAULT,
            ENUM_CURRENT_SETTINGS, HDC, HMONITOR, HORZRES, MONITORINFO, MONITORINFOEXW,
            MONITOR_DEFAULTTONULL,
        },
        UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
    },
};

use crate::error::{XCapError, XCapResult};

use super::{boxed::BoxHDC, capture::capture_monitor, utils::wide_string_to_string};

// A 函数与 W 函数区别
// https://learn.microsoft.com/zh-cn/windows/win32/learnwin32/working-with-strings

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    #[allow(unused)]
    pub hmonitor: HMONITOR,
    #[allow(unused)]
    pub monitor_info_ex_w: MONITORINFOEXW,
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

extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    state: LPARAM,
) -> BOOL {
    unsafe {
        let state = Box::leak(Box::from_raw(state.0 as *mut Vec<HMONITOR>));
        state.push(hmonitor);

        TRUE
    }
}

fn get_dev_mode_w(monitor_info_exw: &MONITORINFOEXW) -> XCapResult<DEVMODEW> {
    let sz_device = monitor_info_exw.szDevice.as_ptr();
    let mut dev_mode_w = DEVMODEW::default();
    dev_mode_w.dmSize = mem::size_of::<DEVMODEW>() as u16;

    unsafe {
        EnumDisplaySettingsW(PCWSTR(sz_device), ENUM_CURRENT_SETTINGS, &mut dev_mode_w).ok()?;
    };

    Ok(dev_mode_w)
}

impl ImplMonitor {
    pub fn new(hmonitor: HMONITOR) -> XCapResult<ImplMonitor> {
        let mut monitor_info_ex_w = MONITORINFOEXW::default();
        monitor_info_ex_w.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
        let monitor_info_ex_w_ptr =
            &mut monitor_info_ex_w as *mut MONITORINFOEXW as *mut MONITORINFO;

        // https://learn.microsoft.com/zh-cn/windows/win32/api/winuser/nf-winuser-getmonitorinfoa
        unsafe { GetMonitorInfoW(hmonitor, monitor_info_ex_w_ptr).ok()? };

        let dev_mode_w = get_dev_mode_w(&monitor_info_ex_w)?;

        let dm_position = unsafe { dev_mode_w.Anonymous1.Anonymous2.dmPosition };
        let dm_pels_width = dev_mode_w.dmPelsWidth;
        let dm_pels_height = dev_mode_w.dmPelsHeight;

        let dm_display_orientation =
            unsafe { dev_mode_w.Anonymous1.Anonymous2.dmDisplayOrientation };
        let rotation = match dm_display_orientation {
            DMDO_90 => 90.0,
            DMDO_180 => 180.0,
            DMDO_270 => 270.0,
            DMDO_DEFAULT => 0.0,
            _ => 0.0,
        };

        let box_hdc_monitor = BoxHDC::from(&monitor_info_ex_w.szDevice);

        let scale_factor = unsafe {
            let physical_width = GetDeviceCaps(*box_hdc_monitor, DESKTOPHORZRES);
            let logical_width = GetDeviceCaps(*box_hdc_monitor, HORZRES);

            physical_width as f32 / logical_width as f32
        };

        Ok(ImplMonitor {
            hmonitor,
            monitor_info_ex_w,
            id: hmonitor.0 as u32,
            name: wide_string_to_string(&monitor_info_ex_w.szDevice)?,
            x: dm_position.x,
            y: dm_position.y,
            width: dm_pels_width,
            height: dm_pels_height,
            rotation,
            scale_factor,
            frequency: dev_mode_w.dmDisplayFrequency as f32,
            is_primary: monitor_info_ex_w.monitorInfo.dwFlags == MONITORINFOF_PRIMARY,
        })
    }

    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        let hmonitors_mut_ptr: *mut Vec<HMONITOR> = Box::into_raw(Box::default());

        let hmonitors = unsafe {
            EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(monitor_enum_proc),
                LPARAM(hmonitors_mut_ptr as isize),
            )
            .ok()?;
            Box::from_raw(hmonitors_mut_ptr)
        };

        let mut impl_monitors = Vec::with_capacity(hmonitors.len());

        for &hmonitor in hmonitors.iter() {
            impl_monitors.push(ImplMonitor::new(hmonitor)?);
        }

        Ok(impl_monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<ImplMonitor> {
        let point = POINT { x, y };
        let hmonitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };

        if hmonitor.is_invalid() {
            return Err(XCapError::new("Not found monitor"));
        }

        ImplMonitor::new(hmonitor)
    }
}

impl ImplMonitor {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture_monitor(self.x, self.y, self.width as i32, self.height as i32)
    }
}
