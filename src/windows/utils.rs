use std::mem;

use image::RgbaImage;
use windows::{
    core::{s, w, HRESULT},
    Win32::{
        Foundation::{GetLastError, HANDLE},
        System::{
            LibraryLoader::GetProcAddress,
            Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
        },
    },
};

use crate::{error::XCapResult, XCapError};

use super::boxed::BoxHModule;

pub(super) fn wide_string_to_string(wide_string: &[u16]) -> XCapResult<String> {
    let string = if let Some(null_pos) = wide_string.iter().position(|pos| *pos == 0) {
        String::from_utf16(&wide_string[..null_pos])?
    } else {
        String::from_utf16(wide_string)?
    };

    Ok(string)
}

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

        let build_version = wide_string_to_string(&buf).unwrap_or_default();

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

pub(super) fn log_last_error<T: ToString>(label: T) {
    unsafe {
        let err = GetLastError();
        log::error!("{} error: {:?}", label.to_string(), err);
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
        let box_hmodule = BoxHModule::new(w!("Shcore.dll"))?;

        let get_process_dpi_awareness_proc_address =
            GetProcAddress(*box_hmodule, s!("GetProcessDpiAwareness")).ok_or(XCapError::new(
                "GetProcAddress GetProcessDpiAwareness failed",
            ))?;

        let get_process_dpi_awareness: GetProcessDpiAwareness =
            mem::transmute(get_process_dpi_awareness_proc_address);

        let mut process_dpi_awareness = 0;
        // https://learn.microsoft.com/zh-cn/windows/win32/api/shellscalingapi/nf-shellscalingapi-getprocessdpiawareness
        get_process_dpi_awareness(process, &mut process_dpi_awareness).ok()?;

        // 当前进程不感知 DPI，则回退到 GetDeviceCaps 获取 DPI
        Ok(process_dpi_awareness != 0)
    }
}
