use image::RgbaImage;
use xcb::{
    Xid,
    x::{
        ATOM_ATOM, ATOM_CARDINAL, ATOM_NONE, ATOM_STRING, ATOM_WM_CLASS, ATOM_WM_NAME, Atom,
        Drawable, GetGeometry, GetProperty, GetPropertyReply, QueryPointer, TranslateCoordinates,
        Window,
    },
};

use crate::error::{XCapError, XCapResult};

use super::{
    capture::capture_window,
    impl_monitor::ImplMonitor,
    utils::{get_atom, get_xcb_connection_and_index},
};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub window: Window,
}

fn get_window_property(
    window: Window,
    property: Atom,
    r#type: Atom,
    long_offset: u32,
    long_length: u32,
) -> XCapResult<GetPropertyReply> {
    let (conn, _) = get_xcb_connection_and_index()?;

    let window_property_cookie = conn.send_request(&GetProperty {
        delete: false,
        window,
        property,
        r#type,
        long_offset,
        long_length,
    });

    let window_property_reply = conn.wait_for_reply(window_property_cookie)?;

    Ok(window_property_reply)
}

pub fn get_window_pid(window: &Window) -> XCapResult<u32> {
    let wm_pid_atom = get_atom("_NET_WM_PID")?;

    let reply = get_window_property(*window, wm_pid_atom, ATOM_CARDINAL, 0, 4)?;
    let value = reply.value::<u32>();

    value
        .first()
        .ok_or(XCapError::new("Get window pid failed"))
        .copied()
}

fn get_active_window_id() -> XCapResult<u32> {
    let (conn, _) = get_xcb_connection_and_index()?;
    let active_window_atom = get_atom("_NET_ACTIVE_WINDOW")?;
    let setup = conn.get_setup();

    for screen in setup.roots() {
        let root_window = screen.root();
        let active_window_id =
            get_window_property(root_window, active_window_atom, ATOM_NONE, 0, 4)?;
        if let Some(&active_window_id) = active_window_id.value::<u32>().first() {
            return Ok(active_window_id);
        }
    }

    Err(XCapError::new("Get active window id failed"))
}

fn get_position_and_size(window: &Window) -> XCapResult<(i32, i32, u32, u32)> {
    let (conn, _) = get_xcb_connection_and_index()?;
    let get_geometry_cookie = conn.send_request(&GetGeometry {
        drawable: Drawable::Window(*window),
    });
    let get_geometry_reply = conn.wait_for_reply(get_geometry_cookie)?;

    let translate_coordinates_cookie = conn.send_request(&TranslateCoordinates {
        dst_window: get_geometry_reply.root(),
        src_window: *window,
        src_x: get_geometry_reply.x(),
        src_y: get_geometry_reply.y(),
    });
    let translate_coordinates_reply = conn.wait_for_reply(translate_coordinates_cookie)?;

    Ok((
        (translate_coordinates_reply.dst_x() - get_geometry_reply.x()) as i32,
        (translate_coordinates_reply.dst_y() - get_geometry_reply.y()) as i32,
        get_geometry_reply.width() as u32,
        get_geometry_reply.height() as u32,
    ))
}

fn get_window_state(window: &Window) -> XCapResult<(bool, bool)> {
    // https://specifications.freedesktop.org/wm-spec/1.3/ar01s05.html
    let wm_state_atom = get_atom("_NET_WM_STATE")?;
    let wm_state_hidden_atom = get_atom("_NET_WM_STATE_HIDDEN")?;
    let wm_state_maximized_vert_atom = get_atom("_NET_WM_STATE_MAXIMIZED_VERT")?;
    let wm_state_maximized_horz_atom = get_atom("_NET_WM_STATE_MAXIMIZED_HORZ")?;

    let wm_state_reply = get_window_property(*window, wm_state_atom, ATOM_ATOM, 0, 12)?;
    let wm_state = wm_state_reply.value::<Atom>();

    let is_minimized = wm_state.contains(&wm_state_hidden_atom);

    let is_maximized_vert = wm_state.contains(&wm_state_maximized_vert_atom);

    let is_maximized_horz = wm_state.contains(&wm_state_maximized_horz_atom);

    Ok((
        is_minimized,
        !is_minimized && is_maximized_vert && is_maximized_horz,
    ))
}

impl ImplWindow {
    fn new(window: Window) -> ImplWindow {
        ImplWindow { window }
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        let (conn, _) = get_xcb_connection_and_index()?;

        let setup = conn.get_setup();

        // https://github.com/rust-x-bindings/rust-xcb/blob/main/examples/get_all_windows.rs
        // https://specifications.freedesktop.org/wm-spec/1.5/ar01s03.html#id-1.4.4
        // list all windows by stacking order
        let client_list_atom = get_atom("_NET_CLIENT_LIST_STACKING")?;

        let mut impl_windows = Vec::new();

        for screen in setup.roots() {
            let root_window = screen.root();

            let query_pointer_cookie = conn.send_request(&QueryPointer {
                window: root_window,
            });
            let query_pointer_reply = match conn.wait_for_reply(query_pointer_cookie) {
                Ok(query_pointer_reply) => query_pointer_reply,
                _ => continue,
            };

            if query_pointer_reply.same_screen() {
                let list_window_reply =
                    match get_window_property(root_window, client_list_atom, ATOM_NONE, 0, 1024) {
                        Ok(list_window_reply) => list_window_reply,
                        _ => continue,
                    };

                for &window in list_window_reply.value::<Window>() {
                    impl_windows.push(ImplWindow::new(window));
                }
            }
        }

        // 按照z轴顺序排序，z值越大，窗口越靠前
        impl_windows.reverse();

        Ok(impl_windows)
    }
}

impl ImplWindow {
    pub fn id(&self) -> XCapResult<u32> {
        Ok(self.window.resource_id())
    }

    pub fn pid(&self) -> XCapResult<u32> {
        get_window_pid(&self.window)
    }

    pub fn app_name(&self) -> XCapResult<String> {
        let get_class_reply =
            get_window_property(self.window, ATOM_WM_CLASS, ATOM_STRING, 0, 1024)?;

        let wm_class = String::from_utf8(get_class_reply.value().to_vec())?;

        // WM_CLASS contains two strings: instance name and class name
        // We want the class name (second string)
        let app_name = wm_class
            .split('\u{0}')
            .nth(1) // Take the second string (class name)
            .unwrap_or("")
            .to_string();

        Ok(app_name)
    }

    pub fn title(&self) -> XCapResult<String> {
        // First try _NET_WM_NAME with UTF8_STRING type
        let net_wm_name_atom = get_atom("_NET_WM_NAME")?;
        let utf8_string_atom = get_atom("UTF8_STRING")?;
        let get_title_reply =
            get_window_property(self.window, net_wm_name_atom, utf8_string_atom, 0, 1024)?;
        let title = String::from_utf8(get_title_reply.value().to_vec())?;

        // If _NET_WM_NAME is empty, fall back to WM_NAME with COMPOUND_TEXT type
        if title.is_empty() {
            let compound_text_atom = get_atom("COMPOUND_TEXT")?;
            let get_title_reply =
                get_window_property(self.window, ATOM_WM_NAME, compound_text_atom, 0, 1024)?;
            let title = String::from_utf8(get_title_reply.value().to_vec())?;

            // If both are empty, try to get the parent window
            if title.is_empty() {
                let (conn, _) = get_xcb_connection_and_index()?;
                let query_tree_cookie = conn.send_request(&xcb::x::QueryTree {
                    window: self.window,
                });
                if let Ok(query_tree_reply) = conn.wait_for_reply(query_tree_cookie) {
                    let parent = query_tree_reply.parent();
                    if parent.resource_id() != 0 {
                        // Try to get title from parent window
                        let parent_window = ImplWindow::new(parent);
                        return parent_window.title();
                    }
                }
            }

            return Ok(title);
        }

        Ok(title)
    }

    pub fn current_monitor(&self) -> XCapResult<ImplMonitor> {
        let impl_monitors = ImplMonitor::all()?;
        let mut find_result = impl_monitors
            .first()
            .ok_or(XCapError::new("Get screen info failed"))?
            .to_owned();

        let (x, y, width, height) = get_position_and_size(&self.window)?;

        let mut max_area = 0;
        // window与哪一个monitor交集最大就属于那个monitor
        for impl_monitor in impl_monitors {
            let monitor_x = impl_monitor.x()?;
            let monitor_y = impl_monitor.y()?;
            let monitor_width = impl_monitor.width()?;
            let monitor_height = impl_monitor.height()?;

            let left = x.max(monitor_x);
            let top = y.max(monitor_x);
            let right = (x + width as i32).min(monitor_x + monitor_width as i32);
            let bottom = (y + height as i32).min(monitor_y + monitor_height as i32);

            // 与0比较，如果小于0则表示两个矩形无交集
            let width = (right - left).max(0);
            let height = (bottom - top).max(0);

            let overlap_area = width * height;
            // 获取最大的面积
            if overlap_area > max_area {
                max_area = overlap_area;
                find_result = impl_monitor;
            }
        }

        Ok(find_result)
    }

    pub fn x(&self) -> XCapResult<i32> {
        let (x, _, _, _) = get_position_and_size(&self.window)?;

        Ok(x)
    }

    pub fn y(&self) -> XCapResult<i32> {
        let (_, y, _, _) = get_position_and_size(&self.window)?;

        Ok(y)
    }

    pub fn z(&self) -> XCapResult<i32> {
        let impl_windows = ImplWindow::all()?;
        let mut z = impl_windows.len() as i32;
        for impl_window in impl_windows {
            z -= 1;
            if impl_window.window == self.window {
                break;
            }
        }

        Ok(z)
    }

    pub fn width(&self) -> XCapResult<u32> {
        let (_, _, width, _) = get_position_and_size(&self.window)?;

        Ok(width)
    }

    pub fn height(&self) -> XCapResult<u32> {
        let (_, _, _, height) = get_position_and_size(&self.window)?;

        Ok(height)
    }

    pub fn is_minimized(&self) -> XCapResult<bool> {
        let (is_minimized, _) = get_window_state(&self.window)?;

        Ok(is_minimized)
    }

    pub fn is_maximized(&self) -> XCapResult<bool> {
        let (is_minimized, _) = get_window_state(&self.window)?;

        Ok(is_minimized)
    }

    pub fn is_focused(&self) -> XCapResult<bool> {
        let active_window_id = get_active_window_id()?;

        Ok(active_window_id == self.id()?)
    }

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture_window(self)
    }
}
