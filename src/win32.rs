use crate::{Image, ScreenCapturer};
use sfhash::digest;
use std::{mem, ptr};
use widestring::U16CString;
use windows::{
  core::PCWSTR,
  Win32::{
    Foundation::{BOOL, LPARAM, RECT},
    Graphics::Gdi::{
      CreateCompatibleBitmap, CreateCompatibleDC, CreateDCW, CreatedHDC, DeleteDC, DeleteObject,
      EnumDisplayMonitors, GetDIBits, GetMonitorInfoW, GetObjectW, SelectObject, SetStretchBltMode,
      StretchBlt, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC, HMONITOR,
      MONITORINFOEXW, RGBQUAD, SRCCOPY, STRETCH_HALFTONE,
    },
  },
};

fn get_monitor_info_exw(h_monitor: HMONITOR) -> Option<MONITORINFOEXW> {
  unsafe {
    let mut monitor_info_exw: MONITORINFOEXW = mem::zeroed();
    monitor_info_exw.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    let monitor_info_exw_ptr = <*mut _>::cast(&mut monitor_info_exw);

    match GetMonitorInfoW(h_monitor, monitor_info_exw_ptr) {
      BOOL(0) => None,
      _ => Some(monitor_info_exw),
    }
  }
}

fn get_monitor_info_exw_from_id<'a>(id: u32) -> Option<MONITORINFOEXW> {
  unsafe {
    let monitor_info_exws = Box::into_raw(Box::new(Vec::<MONITORINFOEXW>::new()));

    match EnumDisplayMonitors(
      HDC::default(),
      ptr::null_mut(),
      Some(monitor_enum_proc),
      LPARAM(monitor_info_exws as isize),
    ) {
      BOOL(0) => None,
      _ => {
        let monitor_info_exws_borrow = &Box::from_raw(monitor_info_exws);
        let monitor_info_exw = monitor_info_exws_borrow.iter().find(|&&monitor_info_exw| {
          let sz_device_ptr = monitor_info_exw.szDevice.as_ptr();
          let sz_device_string = U16CString::from_ptr_str(sz_device_ptr).to_string_lossy();
          digest(sz_device_string.as_bytes()) == id
        })?;

        Some(*monitor_info_exw)
      }
    }
  }
}

extern "system" fn monitor_enum_proc(
  h_monitor: HMONITOR,
  _: HDC,
  _: *mut RECT,
  state: LPARAM,
) -> BOOL {
  unsafe {
    let state = Box::leak(Box::from_raw(state.0 as *mut Vec<MONITORINFOEXW>));

    match get_monitor_info_exw(h_monitor) {
      Some(monitor_info_exw) => {
        state.push(monitor_info_exw);
        BOOL::from(true)
      }
      None => BOOL::from(false),
    }
  }
}

pub fn capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  unsafe {
    let display_info = screen_capturer.display_info;

    let monitor_info_exw = get_monitor_info_exw_from_id(display_info.id)?;

    let sz_device = monitor_info_exw.szDevice;

    let width = (display_info.width as f32 * display_info.scale) as i32;
    let height = (display_info.height as f32 * display_info.scale) as i32;

    let h_dc = CreateDCW(
      PCWSTR(sz_device.as_ptr()),
      PCWSTR(sz_device.as_ptr()),
      PCWSTR(ptr::null()),
      ptr::null(),
    );

    let compatible_dc = CreateCompatibleDC(h_dc);
    let h_bitmap = CreateCompatibleBitmap(h_dc, width, height);

    let release_data = |(h_dc, compatible_dc, h_bitmap): (CreatedHDC, CreatedHDC, HBITMAP)| {
      DeleteDC(h_dc);
      DeleteDC(compatible_dc);
      DeleteObject(h_bitmap);
    };

    SelectObject(compatible_dc, h_bitmap);
    SetStretchBltMode(h_dc, STRETCH_HALFTONE);

    let stretch_blt_result = StretchBlt(
      compatible_dc,
      0,
      0,
      width,
      height,
      h_dc,
      0,
      0,
      width,
      height,
      SRCCOPY,
    );

    if !stretch_blt_result.as_bool() {
      release_data((h_dc, compatible_dc, h_bitmap));
      return None;
    }

    let mut bitmap_info = BITMAPINFO {
      bmiHeader: BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: height, // 这里可以传递负数, 但是不知道为什么会报错
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
      },
      bmiColors: [RGBQUAD::default(); 1],
    };

    let data = vec![0u8; (width * height) as usize * 4];
    let buf_prt = data.as_ptr() as *mut _;

    if GetDIBits(
      compatible_dc,
      h_bitmap,
      0,
      height as u32,
      buf_prt,
      &mut bitmap_info,
      DIB_RGB_COLORS,
    ) == 0
    {
      release_data((h_dc, compatible_dc, h_bitmap));
      return None;
    }

    let mut bitmap = BITMAP::default();
    let bitmap_ptr = <*mut _>::cast(&mut bitmap);

    // Get the BITMAP from the HBITMAP.
    GetObjectW(h_bitmap, mem::size_of::<BITMAP>() as i32, bitmap_ptr);

    // 旋转图像,图像数据是倒置的
    let mut chunks: Vec<Vec<u8>> = data
      .chunks(width as usize * 4)
      .map(|x| x.to_vec())
      .collect();

    chunks.reverse();

    release_data((h_dc, compatible_dc, h_bitmap));

    Some(Image {
      width: bitmap.bmWidth as u32,
      height: bitmap.bmHeight as u32,
      bytes: chunks.concat(),
    })
  }
}
