use image::RgbaImage;
use std::str;
use xcb::{
    x::{
        Atom, Drawable, GetGeometry, GetProperty, GetPropertyReply, InternAtom, QueryPointer,
        TranslateCoordinates, Window, ATOM_ATOM, ATOM_NONE, ATOM_STRING, ATOM_WINDOW,
        ATOM_WM_CLASS, ATOM_WM_NAME,
    },
    Connection, Xid,
};

use crate::error::{XCapError, XCapResult};

use super::{capture::capture_window, impl_monitor::ImplMonitor, utils::Rect};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow {
    pub window: Window,
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub current_monitor: ImplMonitor,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_focused: bool,
}

fn get_atom(conn: &Connection, name: &str) -> XCapResult<Atom> {
    let atom_cookie = conn.send_request(&InternAtom {
        only_if_exists: true,
        name: name.as_bytes(),
    });
    let atom_reply = conn.wait_for_reply(atom_cookie)?;
    let atom = atom_reply.atom();

    if atom == ATOM_NONE {
        return Err(XCapError::new(format!("{} not supported", name)));
    }

    Ok(atom)
}

fn get_window_property(
    conn: &Connection,
    window: Window,
    property: Atom,
    r#type: Atom,
    long_offset: u32,
    long_length: u32,
) -> XCapResult<GetPropertyReply> {
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

fn get_focused_window(conn: &Connection, root_window: Window) -> XCapResult<Window> {
    let active_window_atom = get_atom(conn, "_NET_ACTIVE_WINDOW")?;

    let active_window_reply =
        get_window_property(conn, root_window, active_window_atom, ATOM_WINDOW, 0, 1)?;

    let active_window = active_window_reply
        .value::<Window>()
        .first()
        .copied()
        .unwrap_or(Window::none());

    Ok(active_window)
}

impl ImplWindow {
    fn new(
        conn: &Connection,
        window: &Window,
        impl_monitors: &Vec<ImplMonitor>,
    ) -> XCapResult<ImplWindow> {
        let title = {
            let get_title_reply =
                get_window_property(conn, *window, ATOM_WM_NAME, ATOM_STRING, 0, 1024)?;
            str::from_utf8(get_title_reply.value())?.to_string()
        };

        let app_name = {
            let get_class_reply =
                get_window_property(conn, *window, ATOM_WM_CLASS, ATOM_STRING, 0, 1024)?;

            let class = str::from_utf8(get_class_reply.value())?;

            class
                .split('\u{0}')
                .find(|str| !str.is_empty())
                .unwrap_or("")
                .to_string()
        };

        let (x, y, width, height) = {
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

            (
                (translate_coordinates_reply.dst_x() - get_geometry_reply.x()) as i32,
                (translate_coordinates_reply.dst_y() - get_geometry_reply.y()) as i32,
                get_geometry_reply.width() as u32,
                get_geometry_reply.height() as u32,
            )
        };

        let current_monitor = {
            let mut max_area = 0;
            let mut find_result = impl_monitors
                .first()
                .ok_or(XCapError::new("Get screen info failed"))?;

            let window_rect = Rect::new(x, y, width, height);

            // window与哪一个monitor交集最大就属于那个monitor
            for impl_monitor in impl_monitors {
                let monitor_rect = Rect::new(
                    impl_monitor.x,
                    impl_monitor.y,
                    impl_monitor.width,
                    impl_monitor.height,
                );

                // 获取最大的面积
                let area = window_rect.overlap_area(monitor_rect);
                if area > max_area {
                    max_area = area;
                    find_result = impl_monitor;
                }
            }

            find_result.to_owned()
        };

        let (is_minimized, is_maximized) = {
            // https://specifications.freedesktop.org/wm-spec/1.3/ar01s05.html
            let wm_state_atom = get_atom(conn, "_NET_WM_STATE")?;
            let wm_state_hidden_atom = get_atom(conn, "_NET_WM_STATE_HIDDEN")?;
            let wm_state_maximized_vert_atom = get_atom(conn, "_NET_WM_STATE_MAXIMIZED_VERT")?;
            let wm_state_maximized_horz_atom = get_atom(conn, "_NET_WM_STATE_MAXIMIZED_HORZ")?;

            let wm_state_reply =
                get_window_property(conn, *window, wm_state_atom, ATOM_ATOM, 0, 12)?;
            let wm_state = wm_state_reply.value::<Atom>();

            let is_minimized = wm_state.iter().any(|&state| state == wm_state_hidden_atom);

            let is_maximized_vert = wm_state
                .iter()
                .any(|&state| state == wm_state_maximized_vert_atom);

            let is_maximized_horz = wm_state
                .iter()
                .any(|&state| state == wm_state_maximized_horz_atom);

            (
                is_minimized,
                !is_minimized && is_maximized_vert && is_maximized_horz,
            )
        };

        let is_focused = {
            let setup = conn.get_setup();
            let screen = setup
                .roots()
                .next()
                .ok_or(XCapError::new("No screen found"))?;
            let focused_window = get_focused_window(conn, screen.root())?;
            focused_window == *window
        };

        Ok(ImplWindow {
            window: *window,
            id: window.resource_id(),
            title,
            app_name,
            current_monitor,
            x,
            y,
            width,
            height,
            is_minimized,
            is_maximized,
            is_focused,
        })
    }

    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        let (conn, _) = Connection::connect(None)?;
        let setup = conn.get_setup();

        // https://github.com/rust-x-bindings/rust-xcb/blob/main/examples/get_all_windows.rs
        let client_list_atom = get_atom(&conn, "_NET_CLIENT_LIST")?;

        let mut impl_windows = Vec::new();
        let impl_monitors = ImplMonitor::all()?;

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
                let list_window_reply = match get_window_property(
                    &conn,
                    root_window,
                    client_list_atom,
                    ATOM_NONE,
                    0,
                    100,
                ) {
                    Ok(list_window_reply) => list_window_reply,
                    _ => continue,
                };

                for client in list_window_reply.value::<Window>() {
                    if let Ok(impl_window) = ImplWindow::new(&conn, client, &impl_monitors) {
                        impl_windows.push(impl_window);
                    } else {
                        log::error!(
                            "ImplWindow::new(&conn, {:?}, {:?}) failed",
                            client,
                            &impl_monitors
                        );
                    }
                }
            }
        }

        Ok(impl_windows)
    }
}

impl ImplWindow {
    pub fn refresh(&mut self) -> XCapResult<()> {
        let (conn, _) = Connection::connect(None)?;
        let impl_monitors = ImplMonitor::all()?;
        let impl_window = ImplWindow::new(&conn, &self.window, &impl_monitors)?;

        self.window = impl_window.window;
        self.id = impl_window.id;
        self.title = impl_window.title;
        self.app_name = impl_window.app_name;
        self.current_monitor = impl_window.current_monitor;
        self.x = impl_window.x;
        self.y = impl_window.y;
        self.width = impl_window.width;
        self.height = impl_window.height;
        self.is_minimized = impl_window.is_minimized;
        self.is_maximized = impl_window.is_maximized;
        self.is_focused = impl_window.is_focused;

        Ok(())
    }
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture_window(self)
    }
}
