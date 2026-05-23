use std::{mem, ptr, sync::mpsc::Receiver};

use image::RgbaImage;
use scopeguard::guard;
use widestring::U16CString;
use windows::{
    Win32::Foundation::HMODULE,
    Win32::{
        Devices::Display::DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
        Foundation::{GetLastError, LPARAM, POINT, RECT, TRUE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice},
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIAdapter1, IDXGIDevice,
                IDXGIFactory1, IDXGIOutput1, IDXGIOutput5, IDXGIOutput6,
            },
            Gdi::{
                CreateDCW, DESKTOPHORZRES, DEVMODEW, DMDO_90, DMDO_180, DMDO_270, DMDO_DEFAULT,
                DeleteDC, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
                GetDeviceCaps, GetMonitorInfoW, HDC, HMONITOR, HORZRES, MONITOR_DEFAULTTONULL,
                MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
            },
        },
        System::{LibraryLoader::GetProcAddress, Threading::GetCurrentProcess},
        UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
    },
    core::{BOOL, HRESULT, Interface, PCWSTR, s, w},
};

/// Information about DXGI Desktop Duplication format support for a monitor.
#[derive(Debug, Clone)]
pub struct DxgiFormatSupport {
    /// Panel bit depth reported by the driver (8, 10, 12).
    pub bits_per_color: u32,
    /// Peak luminance in nits.
    pub max_luminance_nits: f32,
    /// Black-level luminance in nits.
    pub min_luminance_nits: f32,
    /// Color space name as reported by `IDXGIOutput6::GetDesc1`.
    pub color_space: String,
    /// Each entry is `(format_name, supported)`.
    pub duplication_formats: Vec<(&'static str, bool)>,
}

use crate::{
    HdrImage,
    error::{XCapError, XCapResult},
    video_recorder::Frame,
};

use super::{
    capture::{capture_monitor, capture_monitor_hdr, capture_region_hdr, monitor_is_hdr},
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

        let name = U16CString::from_vec_truncate(monitor_info_ex_w.szDevice).to_string()?;

        Ok(name)
    }

    pub fn friendly_name(&self) -> XCapResult<String> {
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
        capture_monitor(self, None, None, None, None)
    }

    pub fn capture_region(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<RgbaImage> {
        let image = capture_monitor(self, Some(x), Some(y), Some(width), Some(height))?;
        Ok(image)
    }

    /// Capture the full monitor and return raw HDR pixel data.
    ///
    /// On HDR monitors (without the `wgc` feature), this returns scRGB linear f16 pixels.
    /// On SDR monitors, values are in [0, 1] expressed as f16.
    /// Returns [`crate::XCapError::NotSupported`] when built with the `wgc` feature.
    pub fn capture_image_hdr(&self) -> XCapResult<HdrImage> {
        capture_monitor_hdr(self)
    }

    /// Capture a region of the monitor as raw HDR pixel data.
    ///
    /// Coordinates are in physical pixels relative to the top-left of the monitor.
    /// Returns [`crate::XCapError::NotSupported`] when built with the `wgc` feature.
    pub fn capture_region_hdr(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<HdrImage> {
        capture_region_hdr(self, x, y, width, height)
    }

    /// Returns `true` if the monitor is currently in HDR mode.
    ///
    /// Always returns `false` when built with the `wgc` feature.
    pub fn is_hdr(&self) -> bool {
        monitor_is_hdr(self)
    }

    pub fn peak_nits(&self) -> f64 {
        self.dxgi_format_support()
            .map(|f| f.max_luminance_nits as f64)
            .unwrap_or(0.0)
    }

    pub fn video_recorder(&self) -> XCapResult<(ImplVideoRecorder, Receiver<Frame>)> {
        ImplVideoRecorder::new(self.h_monitor)
    }

    pub fn dxgi_format_support(&self) -> Option<DxgiFormatSupport> {
        const HDR_FORMATS: &[(&str, DXGI_FORMAT)] = &[
            ("R16G16B16A16_FLOAT", DXGI_FORMAT_R16G16B16A16_FLOAT),
            ("R10G10B10A2_UNORM", DXGI_FORMAT_R10G10B10A2_UNORM),
        ];

        unsafe {
            let factory = CreateDXGIFactory1::<IDXGIFactory1>().ok()?;

            let mut adapter_idx = 0u32;
            loop {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_idx) {
                    Ok(a) => a,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    _ => break,
                };
                adapter_idx += 1;

                let mut output_idx = 0u32;
                loop {
                    let output = match adapter.EnumOutputs(output_idx) {
                        Ok(o) => o,
                        Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                        _ => break,
                    };
                    output_idx += 1;

                    let Ok(desc) = output.GetDesc() else { continue };
                    if desc.Monitor != self.h_monitor {
                        continue;
                    }

                    let (bits_per_color, max_luminance_nits, min_luminance_nits, color_space) =
                        output
                            .cast::<IDXGIOutput6>()
                            .ok()
                            .and_then(|o6| o6.GetDesc1().ok())
                            .map(|d| {
                                let cs =
                                    match d.ColorSpace.0 {
                                        0  => "RGB_FULL_G22_NONE_P709 (sRGB)".to_string(),
                                        1  => "RGB_FULL_G10_NONE_P709 (scRGB linear / Windows HDR)".to_string(),
                                        2  => "RGB_STUDIO_G22_NONE_P709".to_string(),
                                        3  => "RGB_STUDIO_G22_NONE_P2020".to_string(),
                                        4  => "RESERVED".to_string(),
                                        5  => "YCBCR_FULL_G22_NONE_P709_X601".to_string(),
                                        6  => "YCBCR_STUDIO_G22_LEFT_P601".to_string(),
                                        7  => "YCBCR_FULL_G22_LEFT_P601".to_string(),
                                        8  => "YCBCR_STUDIO_G22_LEFT_P709".to_string(),
                                        9  => "YCBCR_FULL_G22_LEFT_P709".to_string(),
                                        10 => "YCBCR_STUDIO_G22_LEFT_P2020".to_string(),
                                        11 => "YCBCR_FULL_G22_LEFT_P2020".to_string(),
                                        12 => "RGB_FULL_G2084_NONE_P2020 (HDR10 PQ RGB full)".to_string(),
                                        13 => "YCBCR_STUDIO_G2084_LEFT_P2020 (HDR10 YCbCr AMD HDMI)".to_string(),
                                        14 => "RGB_STUDIO_G2084_NONE_P2020 (HDR10 PQ RGB studio)".to_string(),
                                        15 => "YCBCR_STUDIO_GHLG_TOPLEFT_P2020 (HLG YCbCr studio)".to_string(),
                                        16 => "YCBCR_FULL_GHLG_TOPLEFT_P2020 (HLG YCbCr full)".to_string(),
                                        17 => "RGB_STUDIO_G22_TOPLEFT_P2020".to_string(),
                                        18 => "YCBCR_STUDIO_G2084_TOPLEFT_P2020 (HDR10 YCbCr topleft)".to_string(),
                                        v  => format!("DXGI_COLOR_SPACE_TYPE({})", v),
                                    };
                                (d.BitsPerColor, d.MaxLuminance, d.MinLuminance, cs)
                            })
                            .unwrap_or((0, 0.0, 0.0, "IDXGIOutput6 unavailable".to_string()));

                    let mut d3d_device_opt = None;
                    D3D11CreateDevice(
                        Some(&adapter.cast().ok()?),
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut d3d_device_opt),
                        None,
                        None,
                    )
                    .ok()?;
                    let d3d_device = d3d_device_opt?;
                    let dxgi_device = d3d_device.cast::<IDXGIDevice>().ok()?;

                    // Probe HDR formats via DuplicateOutput1, SDR via DuplicateOutput.
                    let sdr_ok = output
                        .cast::<IDXGIOutput1>()
                        .ok()
                        .and_then(|o1: IDXGIOutput1| o1.DuplicateOutput(&dxgi_device).ok())
                        .is_some();
                    let hdr_format_results: Vec<(&str, bool)> = match output.cast::<IDXGIOutput5>() {
                        Err(_) => HDR_FORMATS.iter().map(|(name, _)| (*name, false)).collect(),
                        Ok(output5) => HDR_FORMATS
                            .iter()
                            .map(|(name, fmt)| {
                                let supported = output5
                                    .DuplicateOutput1(&dxgi_device, 0, &[*fmt])
                                    .is_ok();
                                (*name, supported)
                            })
                            .collect(),
                    };
                    let mut duplication_formats = hdr_format_results;
                    duplication_formats.push(("DuplicateOutput (SDR)", sdr_ok));

                    return Some(DxgiFormatSupport {
                        bits_per_color,
                        max_luminance_nits,
                        min_luminance_nits,
                        color_space: color_space.to_string(),
                        duplication_formats,
                    });
                }
            }
            None
        }
    }
}

#[cfg(feature = "wgc")]
impl Drop for ImplMonitor {
    fn drop(&mut self) {
        use super::wgc::MONITOR_GRAPHICS_CAPTURE_ITEM;

        if let Ok(mut monitor_items) = MONITOR_GRAPHICS_CAPTURE_ITEM.lock() {
            monitor_items.remove(&(self.h_monitor.0 as usize));
        }
    }
}
