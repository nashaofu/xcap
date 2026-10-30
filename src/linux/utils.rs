use std::{
    env::{self, var_os},
    path::{Path, PathBuf},
};

use image::{RgbaImage, open};
use percent_encoding::percent_decode_str;
use url::Url;
use xcb::{
    Connection as XcbConnection, Xid,
    randr::{GetMonitors, MonitorInfoBuf, Output},
    x::{Atom, InternAtom, ScreenBuf},
};

use crate::{XCapError, error::XCapResult};

pub fn get_xcb_connection_and_index() -> XCapResult<(XcbConnection, i32)> {
    let display = env::var("DISPLAY").unwrap_or_else(|_| "DISPLAY:1".to_string());
    let (conn, idx) = XcbConnection::connect(Some(display.as_str()))
        .or_else(|_| XcbConnection::connect(None))
        .map_err(|e| XCapError::new(e.to_string()))?;
    Ok((conn, idx))
}

pub fn wayland_detect() -> bool {
    let xdg_session_type = var_os("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let wayland_display = var_os("WAYLAND_DISPLAY")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    xdg_session_type.eq("wayland") || wayland_display.to_lowercase().contains("wayland")
}

pub fn get_current_screen_buf() -> XCapResult<ScreenBuf> {
    let (conn, index) = get_xcb_connection_and_index()?;

    let setup = conn.get_setup();

    let screen = setup
        .roots()
        .nth(index as usize)
        .ok_or_else(|| XCapError::new("Not found screen"))?;

    Ok(screen.to_owned())
}

pub fn get_monitor_info_buf(output: Output) -> XCapResult<MonitorInfoBuf> {
    let (conn, _) = get_xcb_connection_and_index()?;

    let screen_buf = get_current_screen_buf()?;

    let get_monitors_cookie = conn.send_request(&GetMonitors {
        window: screen_buf.root(),
        get_active: true,
    });

    let get_monitors_reply = conn.wait_for_reply(get_monitors_cookie)?;

    let monitor_info_iterator = get_monitors_reply.monitors();

    for monitor_info in monitor_info_iterator {
        for &item in monitor_info.outputs() {
            if item == output {
                return Ok(monitor_info.to_owned());
            }
        }
    }
    Err(XCapError::new("Not found monitor"))
}

pub fn get_atom(name: &str) -> XCapResult<Atom> {
    let (conn, _) = get_xcb_connection_and_index()?;
    let atom_cookie = conn.send_request(&InternAtom {
        only_if_exists: true,
        name: name.as_bytes(),
    });
    let atom_reply = conn.wait_for_reply(atom_cookie)?;
    let atom = atom_reply.atom();

    if atom.is_none() {
        return Err(XCapError::new(format!("{name} not supported")));
    }

    Ok(atom)
}

pub(super) fn png_to_rgba_image<T>(
    filename: T,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage>
where
    T: AsRef<Path>,
{
    let mut dynamic_image = open(filename)?;
    dynamic_image = dynamic_image.crop(x as u32, y as u32, width as u32, height as u32);
    Ok(dynamic_image.to_rgba8())
}

/// uri 转换为 path
pub(super) fn safe_uri_to_path(uri: &str) -> XCapResult<PathBuf> {
    let url = Url::parse(uri)?;

    if url.scheme() != "file" {
        return Err(XCapError::new("Uri scheme is not file"));
    }

    // 获取已解码的路径
    let decoded_path = percent_decode_str(url.path())
        .decode_utf8_lossy()
        .to_string();

    let path = PathBuf::from(&decoded_path);

    Ok(path)
}
