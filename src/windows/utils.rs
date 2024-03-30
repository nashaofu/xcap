use sysinfo::System;
use windows::Win32::{
    Foundation::{GetLastError, HWND, RECT},
    UI::WindowsAndMessaging::GetWindowRect,
};

use crate::error::XCapResult;

pub(super) fn wide_string_to_string(wide_string: &[u16]) -> XCapResult<String> {
    let string = if let Some(null_pos) = wide_string.iter().position(|pos| *pos == 0) {
        String::from_utf16(&wide_string[..null_pos])?
    } else {
        String::from_utf16(wide_string)?
    };

    Ok(string)
}

pub(super) fn get_os_major_version() -> u8 {
    System::os_version()
        .map(|os_version| {
            let strs: Vec<&str> = os_version.split(' ').collect();
            strs[0].parse::<u8>().unwrap_or(0)
        })
        .unwrap_or(0)
}

pub(super) fn get_window_rect(hwnd: HWND) -> XCapResult<RECT> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect)?;
        Ok(rect)
    }
}

pub(super) fn log_last_error<T: ToString>(label: T) {
    unsafe {
        let err = GetLastError();
        log::error!("{} error: {:?}", label.to_string(), err);
    }
}
