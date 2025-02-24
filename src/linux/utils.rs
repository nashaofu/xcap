use image::{open, RgbaImage};
use lazy_static::lazy_static;
use xcb::{
    randr::{GetMonitors, MonitorInfoBuf, Output},
    x::ScreenBuf,
    ConnResult, Connection,
};

use crate::{error::XCapResult, XCapError};

lazy_static! {
    static ref XCB_CONNECTION_AND_INDEX: ConnResult<(Connection, i32)> =
        xcb::Connection::connect(None);
}

pub fn get_xcb_connection_and_index() -> XCapResult<&'static (Connection, i32)> {
    XCB_CONNECTION_AND_INDEX
        .as_ref()
        .map_err(|err| XCapError::new(err))
}

pub fn get_current_screen_buf() -> XCapResult<ScreenBuf> {
    let (conn, index) = get_xcb_connection_and_index()?;

    let setup = conn.get_setup();

    let screen = setup
        .roots()
        .nth(*index as usize)
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

pub(super) fn png_to_rgba_image(
    filename: &String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let mut dynamic_image = open(filename)?;
    dynamic_image = dynamic_image.crop(x as u32, y as u32, width as u32, height as u32);
    Ok(dynamic_image.to_rgba8())
}
