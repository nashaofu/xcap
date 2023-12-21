use image::RgbaImage;
use std::mem;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{BOOL, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::{
            EnumDisplayMonitors, EnumDisplaySettingsExW, GetDeviceCaps, GetMonitorInfoW,
            MonitorFromPoint, DESKTOPHORZRES, DEVMODEW, DEVMODE_DISPLAY_ORIENTATION, EDS_RAWMODE,
            ENUM_CURRENT_SETTINGS, HDC, HMONITOR, HORZRES, MONITORINFO, MONITORINFOEXW,
            MONITOR_DEFAULTTONULL,
        },
        UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
    },
};

use crate::error::{XCapError, XCapResult};

use super::{boxed::BoxHDC, capture::capture_monitor, utils::wide_string_to_string};

// A函数与W函数区别
// https://learn.microsoft.com/zh-cn/windows/win32/learnwin32/working-with-strings

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    #[allow(unused)]
    pub hmonitor: HMONITOR,
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
        EnumDisplaySettingsExW(
            PCWSTR(sz_device),
            ENUM_CURRENT_SETTINGS,
            &mut dev_mode_w,
            EDS_RAWMODE,
        )
        .ok()?;
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
        let rc_monitor = monitor_info_ex_w.monitorInfo.rcMonitor;

        let dev_mode_w = get_dev_mode_w(&monitor_info_ex_w)?;

        let dm_display_orientation =
            unsafe { dev_mode_w.Anonymous1.Anonymous2.dmDisplayOrientation };

        let rotation = match dm_display_orientation {
            DEVMODE_DISPLAY_ORIENTATION(0) => 0.0,
            DEVMODE_DISPLAY_ORIENTATION(1) => 90.0,
            DEVMODE_DISPLAY_ORIENTATION(2) => 180.0,
            DEVMODE_DISPLAY_ORIENTATION(3) => 270.0,
            _ => dm_display_orientation.0 as f32,
        };

        let dev_mode_w = get_dev_mode_w(&monitor_info_ex_w)?;

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
            x: rc_monitor.left,
            y: rc_monitor.top,
            width: (rc_monitor.right - rc_monitor.left) as u32,
            height: (rc_monitor.bottom - rc_monitor.top) as u32,
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
        let width = ((self.width as f32) * self.scale_factor) as i32;
        let height = ((self.height as f32) * self.scale_factor) as i32;

        capture_monitor(self, 0, 0, width, height)
    }
}
