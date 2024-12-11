use image::RgbaImage;
use std::str;
use xcb::{
    randr::{
        GetCrtcInfo, GetMonitors, GetOutputInfo, GetScreenResources, Mode, ModeFlag, ModeInfo,
        MonitorInfo, MonitorInfoBuf, Output, Rotation,
    },
    x::{GetProperty, Screen, ScreenBuf, ATOM_RESOURCE_MANAGER, ATOM_STRING, CURRENT_TIME},
    Connection, Xid,
};

use crate::error::{XCapError, XCapResult};

use super::{capture::capture_monitor, impl_video_recorder::ImplVideoRecorder};

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    pub screen_buf: ScreenBuf,
    #[allow(unused)]
    pub monitor_info_buf: MonitorInfoBuf,
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rotation: f32,
    pub scale_factor: f32,
    pub frequency: f32,
    pub is_primary: bool,
}

// per https://gitlab.freedesktop.org/xorg/app/xrandr/-/blob/master/xrandr.c#L576
fn get_current_frequency(mode_infos: &[ModeInfo], mode: Mode) -> f32 {
    let mode_info = match mode_infos.iter().find(|m| m.id == mode.resource_id()) {
        Some(mode_info) => mode_info,
        None => return 0.0,
    };

    let vtotal = {
        let mut val = mode_info.vtotal;
        if mode_info.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
            val *= 2;
        }
        if mode_info.mode_flags.contains(ModeFlag::INTERLACE) {
            val /= 2;
        }
        val
    };

    if vtotal != 0 && mode_info.htotal != 0 {
        (mode_info.dot_clock as f32) / (vtotal as f32 * mode_info.htotal as f32)
    } else {
        0.0
    }
}

fn get_scale_factor(conn: &Connection, screen: &Screen) -> XCapResult<f32> {
    let xft_dpi_prefix = "Xft.dpi:\t";

    let get_property_cookie = conn.send_request(&GetProperty {
        delete: false,
        window: screen.root(),
        property: ATOM_RESOURCE_MANAGER,
        r#type: ATOM_STRING,
        long_offset: 0,
        long_length: 60,
    });

    let get_property_reply = conn.wait_for_reply(get_property_cookie)?;

    let resource_manager = str::from_utf8(get_property_reply.value())?;

    let xft_dpi = resource_manager
        .split('\n')
        .find(|s| s.starts_with(xft_dpi_prefix))
        .ok_or_else(|| XCapError::new("Xft.dpi parse failed"))?
        .strip_prefix(xft_dpi_prefix)
        .ok_or_else(|| XCapError::new("Xft.dpi parse failed"))?;

    let dpi = xft_dpi.parse::<f32>().map_err(XCapError::new)?;

    Ok(dpi / 96.0)
}

fn get_rotation_frequency(
    conn: &Connection,
    mode_infos: &[ModeInfo],
    output: &Output,
) -> XCapResult<(f32, f32)> {
    let get_output_info_cookie = conn.send_request(&GetOutputInfo {
        output: *output,
        config_timestamp: 0,
    });

    let get_output_info_reply = conn.wait_for_reply(get_output_info_cookie)?;

    let get_crtc_info_cookie = conn.send_request(&GetCrtcInfo {
        crtc: get_output_info_reply.crtc(),
        config_timestamp: 0,
    });

    let get_crtc_info_reply = conn.wait_for_reply(get_crtc_info_cookie)?;

    let mode = get_crtc_info_reply.mode();

    let rotation = match get_crtc_info_reply.rotation() {
        Rotation::ROTATE_0 => 0.0,
        Rotation::ROTATE_90 => 90.0,
        Rotation::ROTATE_180 => 180.0,
        Rotation::ROTATE_270 => 270.0,
        _ => 0.0,
    };

    let frequency = get_current_frequency(mode_infos, mode);

    Ok((rotation, frequency))
}

impl ImplMonitor {
    fn new(
        conn: &Connection,
        screen: &Screen,
        monitor_info: &MonitorInfo,
        output: &Output,
        rotation: f32,
        scale_factor: f32,
        frequency: f32,
    ) -> XCapResult<ImplMonitor> {
        let get_output_info_cookie = conn.send_request(&GetOutputInfo {
            output: *output,
            config_timestamp: CURRENT_TIME,
        });
        let get_output_info_reply = conn.wait_for_reply(get_output_info_cookie)?;

        Ok(ImplMonitor {
            screen_buf: screen.to_owned(),
            monitor_info_buf: monitor_info.to_owned(),
            id: output.resource_id(),
            name: str::from_utf8(get_output_info_reply.name())?.to_string(),
            x: ((monitor_info.x() as f32) / scale_factor) as i32,
            y: ((monitor_info.y() as f32) / scale_factor) as i32,
            width: ((monitor_info.width() as f32) / scale_factor) as u32,
            height: ((monitor_info.height() as f32) / scale_factor) as u32,
            rotation,
            scale_factor,
            frequency,
            is_primary: monitor_info.primary(),
        })
    }

    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        let (conn, index) = Connection::connect(None)?;

        let setup = conn.get_setup();

        let screen = setup
            .roots()
            .nth(index as usize)
            .ok_or_else(|| XCapError::new("Not found screen"))?;

        let scale_factor = get_scale_factor(&conn, screen).unwrap_or(1.0);

        let get_monitors_cookie = conn.send_request(&GetMonitors {
            window: screen.root(),
            get_active: true,
        });

        let get_monitors_reply = conn.wait_for_reply(get_monitors_cookie)?;

        let monitor_info_iterator = get_monitors_reply.monitors();

        let get_screen_resources_cookie = conn.send_request(&GetScreenResources {
            window: screen.root(),
        });

        let get_screen_resources_reply = conn.wait_for_reply(get_screen_resources_cookie)?;

        let mode_infos = get_screen_resources_reply.modes();

        let mut impl_monitors = Vec::new();

        for monitor_info in monitor_info_iterator {
            let output = match monitor_info.outputs().first() {
                Some(output) => output,
                _ => continue,
            };

            let (rotation, frequency) =
                get_rotation_frequency(&conn, mode_infos, output).unwrap_or((0.0, 0.0));

            if let Ok(impl_monitor) = ImplMonitor::new(
                &conn,
                screen,
                monitor_info,
                output,
                rotation,
                scale_factor,
                frequency,
            ) {
                impl_monitors.push(impl_monitor);
            } else {
                log::error!(
                    "ImplMonitor::new(&conn, {:?}, {:?}, {:?}, {}, {}, {}) failed",
                    screen,
                    monitor_info,
                    output,
                    rotation,
                    scale_factor,
                    frequency
                );
            }
        }

        Ok(impl_monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<ImplMonitor> {
        let impl_monitors = ImplMonitor::all()?;

        let impl_monitor = impl_monitors
            .iter()
            .find(|impl_monitor| {
                x >= impl_monitor.x
                    && x < impl_monitor.x + impl_monitor.width as i32
                    && y >= impl_monitor.y
                    && y < impl_monitor.y + impl_monitor.height as i32
            })
            .ok_or_else(|| XCapError::new("Get screen info failed"))?;

        Ok(impl_monitor.clone())
    }
}

impl ImplMonitor {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture_monitor(self)
    }

    pub fn video_recorder(&self) -> XCapResult<ImplVideoRecorder> {
        ImplVideoRecorder::new()
    }
}
