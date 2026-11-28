use image::RgbaImage;

use crate::{Monitor, error::XCapResult, platform::impl_window::ImplWindow};

#[derive(Debug, Clone)]
pub struct Window {
    pub(crate) impl_window: ImplWindow,
}

impl Window {
    pub(crate) fn new(impl_window: ImplWindow) -> Window {
        Window { impl_window }
    }
}

impl Window {
    /// List all windows, sorted by z coordinate.
    ///
    /// On Windows, windows owned by the calling process are excluded. Use
    /// [`Window::all_with_options`] to include them.
    pub fn all() -> XCapResult<Vec<Window>> {
        Window::all_with_options(WindowOptions::default())
    }

    /// List all windows, sorted by z coordinate, with the given options.
    pub fn all_with_options(options: WindowOptions) -> XCapResult<Vec<Window>> {
        #[cfg(target_os = "windows")]
        let impl_windows = ImplWindow::all_with_options(options)?;
        #[cfg(not(target_os = "windows"))]
        let impl_windows = {
            let _ = options;
            ImplWindow::all()?
        };

        let windows = impl_windows
            .iter()
            .map(|impl_window| Window::new(impl_window.clone()))
            .collect();

        Ok(windows)
    }
}

/// Options for [`Window::all_with_options`].
#[derive(Debug, Clone)]
pub struct WindowOptions {
    /// Exclude windows owned by the calling process from the list. Takes effect
    /// on Windows only; macOS and Linux always include them.
    ///
    /// `GetWindowText*` on a window owned by the calling process is a blocking
    /// call that can deadlock if the caller is the same thread pumping that
    /// window's message loop. Set this to `false` only from a worker thread
    /// that does not block the target window's message loop.
    pub excludes_own_windows: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            excludes_own_windows: true,
        }
    }
}

impl WindowOptions {
    pub fn excludes_own_windows(mut self, excludes_own_windows: bool) -> Self {
        self.excludes_own_windows = excludes_own_windows;
        self
    }
}

impl Window {
    /// The window id
    pub fn id(&self) -> XCapResult<u32> {
        self.impl_window.id()
    }
    /// The window process id
    pub fn pid(&self) -> XCapResult<u32> {
        self.impl_window.pid()
    }
    /// The window app name
    pub fn app_name(&self) -> XCapResult<String> {
        self.impl_window.app_name()
    }
    /// The window title
    pub fn title(&self) -> XCapResult<String> {
        self.impl_window.title()
    }
    /// The window current monitor
    pub fn current_monitor(&self) -> XCapResult<Monitor> {
        Ok(Monitor::new(self.impl_window.current_monitor()?))
    }
    /// The window x coordinate.
    pub fn x(&self) -> XCapResult<i32> {
        self.impl_window.x()
    }
    /// The window y coordinate.
    pub fn y(&self) -> XCapResult<i32> {
        self.impl_window.y()
    }
    /// The window z coordinate.
    pub fn z(&self) -> XCapResult<i32> {
        self.impl_window.z()
    }
    /// The window pixel width.
    pub fn width(&self) -> XCapResult<u32> {
        self.impl_window.width()
    }
    /// The window pixel height.
    pub fn height(&self) -> XCapResult<u32> {
        self.impl_window.height()
    }
    /// The window is minimized.
    pub fn is_minimized(&self) -> XCapResult<bool> {
        self.impl_window.is_minimized()
    }
    /// The window is maximized.
    pub fn is_maximized(&self) -> XCapResult<bool> {
        self.impl_window.is_maximized()
    }
    /// The window is focused.
    pub fn is_focused(&self) -> XCapResult<bool> {
        self.impl_window.is_focused()
    }
}

impl Window {
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        self.impl_window.capture_image()
    }
}
