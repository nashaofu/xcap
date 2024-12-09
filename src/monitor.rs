use image::RgbaImage;

use crate::{error::XCapResult, platform::impl_monitor::ImplMonitor, VideoRecorder};

#[derive(Debug, Clone)]
pub struct Monitor {
    pub(crate) impl_monitor: ImplMonitor,
}

impl Monitor {
    pub(crate) fn new(impl_monitor: ImplMonitor) -> Monitor {
        Monitor { impl_monitor }
    }
}

impl Monitor {
    pub fn all() -> XCapResult<Vec<Monitor>> {
        let monitors = ImplMonitor::all()?
            .iter()
            .map(|impl_monitor| Monitor::new(impl_monitor.clone()))
            .collect();

        Ok(monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<Monitor> {
        let impl_monitor = ImplMonitor::from_point(x, y)?;

        Ok(Monitor::new(impl_monitor))
    }
}

impl Monitor {
    /// Unique identifier associated with the screen.
    pub fn id(&self) -> u32 {
        self.impl_monitor.id
    }
    /// Unique identifier associated with the screen.
    pub fn name(&self) -> &str {
        &self.impl_monitor.name
    }
    /// The screen x coordinate.
    pub fn x(&self) -> i32 {
        self.impl_monitor.x
    }
    /// The screen x coordinate.
    pub fn y(&self) -> i32 {
        self.impl_monitor.y
    }
    /// The screen pixel width.
    pub fn width(&self) -> u32 {
        self.impl_monitor.width
    }
    /// The screen pixel height.
    pub fn height(&self) -> u32 {
        self.impl_monitor.height
    }
    /// Can be 0, 90, 180, 270, represents screen rotation in clock-wise degrees.
    pub fn rotation(&self) -> f32 {
        self.impl_monitor.rotation
    }
    /// Output device's pixel scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.impl_monitor.scale_factor
    }
    /// The screen refresh rate.
    pub fn frequency(&self) -> f32 {
        self.impl_monitor.frequency
    }
    /// Whether the screen is the main screen
    pub fn is_primary(&self) -> bool {
        self.impl_monitor.is_primary
    }
}

impl Monitor {
    /// Capture image of the monitor
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        self.impl_monitor.capture_image()
    }

    pub fn video_recorder(&self) -> XCapResult<VideoRecorder> {
        let impl_video_recorder = self.impl_monitor.video_recorder()?;

        Ok(VideoRecorder::new(impl_video_recorder))
    }
}
