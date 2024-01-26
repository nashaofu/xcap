use std::{ops::Deref, ptr};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CreateDCW, DeleteDC, DeleteObject, GetWindowDC, ReleaseDC, HBITMAP, HDC},
    },
};

pub(super) struct BoxHDC {
    hdc: HDC,
    hwnd: Option<HWND>,
}

impl Deref for BoxHDC {
    type Target = HDC;
    fn deref(&self) -> &Self::Target {
        &self.hdc
    }
}

impl Drop for BoxHDC {
    fn drop(&mut self) {
        // ReleaseDC 与 DeleteDC 的区别
        // https://learn.microsoft.com/zh-cn/windows/win32/api/winuser/nf-winuser-releasedc
        unsafe {
            if let Some(hwnd) = self.hwnd {
                ReleaseDC(hwnd, self.hdc);
            } else {
                DeleteDC(self.hdc);
            }
        };
    }
}

impl BoxHDC {
    pub fn new(hdc: HDC, hwnd: Option<HWND>) -> Self {
        BoxHDC { hdc, hwnd }
    }
}

impl From<&[u16; 32]> for BoxHDC {
    fn from(sz_device: &[u16; 32]) -> Self {
        let sz_device_ptr = sz_device.as_ptr();

        let hdc = unsafe {
            CreateDCW(
                PCWSTR(sz_device_ptr),
                PCWSTR(sz_device_ptr),
                PCWSTR(ptr::null()),
                None,
            )
        };

        BoxHDC::new(hdc, None)
    }
}

impl From<HWND> for BoxHDC {
    fn from(hwnd: HWND) -> Self {
        // GetWindowDC vs GetDC, GetDC 不会绘制窗口边框
        // https://learn.microsoft.com/zh-cn/windows/win32/api/winuser/nf-winuser-getwindowdc
        let hdc = unsafe { GetWindowDC(hwnd) };

        BoxHDC::new(hdc, Some(hwnd))
    }
}

pub(super) struct BoxHBITMAP(HBITMAP);

impl Deref for BoxHBITMAP {
    type Target = HBITMAP;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for BoxHBITMAP {
    fn drop(&mut self) {
        // https://learn.microsoft.com/zh-cn/windows/win32/api/wingdi/nf-wingdi-createcompatiblebitmap
        unsafe {
            DeleteObject(self.0);
        };
    }
}

impl BoxHBITMAP {
    pub fn new(h_bitmap: HBITMAP) -> Self {
        BoxHBITMAP(h_bitmap)
    }
}
