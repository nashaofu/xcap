use crate::{
    error::{XCapError, XCapResult},
    video_recorder::Frame,
};
use image::RgbaImage;
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone)]
pub struct ImplMonitor;

impl ImplMonitor {
    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        Ok(Vec::new())
    }

    pub fn from_point(_x: i32, _y: i32) -> XCapResult<ImplMonitor> {
        Err(XCapError::NotSupported)
    }

    pub fn id(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn name(&self) -> XCapResult<String> {
        Err(XCapError::NotSupported)
    }

    pub fn x(&self) -> XCapResult<i32> {
        Err(XCapError::NotSupported)
    }

    pub fn y(&self) -> XCapResult<i32> {
        Err(XCapError::NotSupported)
    }

    pub fn width(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn height(&self) -> XCapResult<u32> {
        Err(XCapError::NotSupported)
    }

    pub fn rotation(&self) -> XCapResult<f32> {
        Err(XCapError::NotSupported)
    }

    pub fn scale_factor(&self) -> XCapResult<f32> {
        Err(XCapError::NotSupported)
    }

    pub fn frequency(&self) -> XCapResult<f32> {
        Err(XCapError::NotSupported)
    }

    pub fn is_primary(&self) -> XCapResult<bool> {
        Err(XCapError::NotSupported)
    }

    pub fn is_builtin(&self) -> XCapResult<bool> {
        Err(XCapError::NotSupported)
    }

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        Err(XCapError::NotSupported)
    }

    pub fn capture_region(
        &self,
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
    ) -> XCapResult<RgbaImage> {
        Err(XCapError::NotSupported)
    }

    pub fn video_recorder(&self) -> XCapResult<(ImplVideoRecorder, Receiver<Frame>)> {
        Err(XCapError::NotSupported)
    }
}

#[derive(Debug, Clone)]
pub struct ImplWindow;

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

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder;

impl ImplVideoRecorder {
    pub fn start(&self) -> XCapResult<()> {
        Err(XCapError::NotSupported)
    }

    pub fn stop(&self) -> XCapResult<()> {
        Err(XCapError::NotSupported)
    }
}

pub mod impl_monitor {
    pub use super::ImplMonitor;
}

pub mod impl_video_recorder {
    pub use super::ImplVideoRecorder;
}

pub mod impl_window {
    pub use super::ImplWindow;
}
