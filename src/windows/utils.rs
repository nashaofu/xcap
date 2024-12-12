use image::RgbaImage;
use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
use windows::Win32::Foundation::GetLastError;

use crate::{error::XCapResult, XCapError};

pub(super) fn wide_string_to_string(wide_string: &[u16]) -> XCapResult<String> {
    let string = if let Some(null_pos) = wide_string.iter().position(|pos| *pos == 0) {
        String::from_utf16(&wide_string[..null_pos])?
    } else {
        String::from_utf16(wide_string)?
    };

    Ok(string)
}

pub(super) fn get_os_major_version() -> u8 {
    unsafe {
        let mut buf_len: u32 = 4;
        let mut buf = [0u8; 4];
        let err = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentMajorVersionNumber"),
            RRF_RT_REG_DWORD,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut buf_len),
        );
        if err.is_ok() {
            u32::from_le_bytes(buf) as u8
        } else {
            0
        }
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
