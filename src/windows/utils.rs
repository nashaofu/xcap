use std::{ffi::c_void, mem};

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
        Foundation::{CloseHandle, FreeLibrary, GetLastError, HANDLE, HMODULE, HWND, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Resource, ID3D11Texture2D,
            },
            Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::MONITORINFOEXW,
        },
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
            Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS},
        },
    },
    core::{HRESULT, Interface, PCWSTR, s, w},
};

use crate::{Frame, XCapError, error::XCapResult};

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

/**
 * 获取 window 可见的实际宽高，包含标题栏、边框等非客户区的部分
 * 不同于 WINDOWINFO 中的 rcWindow 与 rcClient
 *  - rcWindow 包含了窗口的标题栏和边框等非客户区的部分，与 GetWindowRect 获取的窗口位置和大小一致
 *  - rcClient 只包含了窗口的客户区部分，与 GetClientRect 获取的窗口大小一致，但 GetClientRect 返回left 和 top 都是 0，因为客户区的坐标系是相对于窗口的客户区而言的
 *
 * 获取窗口真实 bounds 必须使用 DwmGetWindowAttribute，原因为：
 * 自 Windows 10 起，桌面窗口管理器（DWM）会为部分窗口添加不可见的 resize 边框（通常 8px 左右），例如 chrome 浏览器的窗口，GetWindowRect 获取的窗口位置和大小包含了这个 resize 边框（eg返回值：-8，-8，2560, 1392）
 */
pub(super) fn get_window_bounds(hwnd: HWND) -> XCapResult<RECT> {
    let mut rect = RECT::default();

    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            mem::size_of::<RECT>() as u32,
        )?;
    }

    Ok(rect)
}

pub(super) fn create_d3d_device(flag: D3D11_CREATE_DEVICE_FLAG) -> XCapResult<ID3D11Device> {
    unsafe {
        let mut d3d_device = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flag,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            None,
        )?;

        let d3d_device = d3d_device.ok_or(XCapError::new("Call D3D11CreateDevice failed"))?;

        Ok(d3d_device)
    }
}

pub(super) fn texture_to_frame(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    source_texture: &ID3D11Texture2D,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<Frame> {
    unsafe {
        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut src_desc);

        // 边界检查（防止越界）
        if x + width > src_desc.Width || y + height > src_desc.Height {
            return Err(XCapError::new("ROI out of bounds"));
        }

        let staging_texture = {
            let mut staging_desc = src_desc;
            staging_desc.Width = width;
            staging_desc.Height = height;
            staging_desc.BindFlags = 0;
            staging_desc.MiscFlags = 0;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

            let mut staging = None;
            d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            staging.ok_or(XCapError::new("CreateTexture2D failed"))?
        };

        // GPU裁剪区域
        let region = D3D11_BOX {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
            front: 0,
            back: 1,
        };

        d3d_context.CopySubresourceRegion(
            Some(&staging_texture.cast()?),
            0,
            0,
            0,
            0,
            Some(&source_texture.cast()?),
            0,
            Some(&region),
        );

        let resource: ID3D11Resource = staging_texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(
            Some(&resource.clone()),
            0,
            D3D11_MAP_READ,
            0,
            Some(&mut mapped),
        )?;

        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let src_ptr = mapped.pData as *const u8;

        for row in 0..height {
            let src_offset = (row * mapped.RowPitch) as usize;
            let dst_offset = (row * width * 4) as usize;

            let src_slice =
                std::slice::from_raw_parts(src_ptr.add(src_offset), (width * 4) as usize);

            bgra[dst_offset..dst_offset + (width * 4) as usize].copy_from_slice(src_slice);
        }

        d3d_context.Unmap(Some(&resource), 0);

        Ok(Frame::new(width, height, bgra_to_rgba(bgra.to_owned())))
    }
}
