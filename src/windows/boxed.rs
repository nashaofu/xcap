use std::{ops::Deref, ptr};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CreateDCW, DeleteDC, DeleteObject, GetWindowDC, HBITMAP, HDC},
    },
};

pub(super) struct BoxHDC(HDC);

impl Deref for BoxHDC {
    type Target = HDC;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for BoxHDC {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        };
    }
}

impl BoxHDC {
    pub fn new(hdc: HDC) -> Self {
        BoxHDC(hdc)
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

        BoxHDC::new(hdc)
    }
}

impl From<HWND> for BoxHDC {
    fn from(hwnd: HWND) -> Self {
        let hdc = unsafe { GetWindowDC(hwnd) };

        BoxHDC::new(hdc)
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
