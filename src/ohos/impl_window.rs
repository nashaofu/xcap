//! OHOS window implementation.
//!
//! OHOS NDK (as of API level 15) does not expose a public window-enumeration
//! API, so all methods that require window-level information return
//! `XCapError::NotSupported` and `Window::all()` returns an empty list.

use image::RgbaImage;

use crate::{
    error::{XCapError, XCapResult},
    platform::impl_monitor::ImplMonitor,
};

#[derive(Debug, Clone)]
pub(crate) struct ImplWindow;

impl ImplWindow {
    pub fn all() -> XCapResult<Vec<ImplWindow>> {
        Ok(Vec::new())
    }

    pub fn id(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn pid(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn app_name(&self) -> XCapResult<String> {
        Err(XCapError::NotSupported)
    }

    pub fn title(&self) -> XCapResult<String> {
        Err(XCapError::NotSupported)
    }

    pub fn current_monitor(&self) -> XCapResult<ImplMonitor> {
        Err(XCapError::NotSupported)
    }

    pub fn x(&self) -> XCapResult<i32> {
        Err(XCapError::NotSupported)
    }

    pub fn y(&self) -> XCapResult<i32> {
        Err(XCapError::NotSupported)
    }

    pub fn z(&self) -> XCapResult<i32> {
        Err(XCapError::NotSupported)
    }

    pub fn width(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn height(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn is_minimized(&self) -> XCapResult<bool> {
        Err(XCapError::NotSupported)
    }

    pub fn is_maximized(&self) -> XCapResult<bool> {
        Err(XCapError::NotSupported)
    }

    pub fn is_focused(&self) -> XCapResult<bool> {
        Err(XCapError::NotSupported)
    }

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        Err(XCapError::NotSupported)
    }
}
