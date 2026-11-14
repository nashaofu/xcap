use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

use crate::{XCapResult, platform::impl_video_recorder::ImplVideoRecorder};

/// A rectangle in monitor units, the same space as [`crate::Monitor::capture_region`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Options for [`crate::Monitor::video_recorder_with_options`].
#[derive(Debug, Clone)]
pub struct VideoRecorderOptions {
    /// Upper bound on the delivered frame rate, clamped to `1..=60`.
    pub fps: u32,
    /// Sub-rectangle of the monitor to record, in monitor units.
    /// `None` records the whole monitor.
    pub region: Option<CaptureRegion>,
    /// Draw the mouse cursor into the frames. macOS only; Windows always
    /// excludes it.
    pub captures_cursor: bool,
    /// Leave out the windows owned by the calling process. macOS only; Windows
    /// callers exclude their own windows themselves.
    pub excludes_own_windows: bool,
}

impl Default for VideoRecorderOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            region: None,
            captures_cursor: true,
            excludes_own_windows: true,
        }
    }
}

impl VideoRecorderOptions {
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps.clamp(1, 60);
        self
    }

    pub fn region(mut self, region: CaptureRegion) -> Self {
        self.region = Some(region);
        self
    }

    pub fn captures_cursor(mut self, captures_cursor: bool) -> Self {
        self.captures_cursor = captures_cursor;
        self
    }

    pub fn excludes_own_windows(mut self, excludes_own_windows: bool) -> Self {
        self.excludes_own_windows = excludes_own_windows;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA pixels, `width * height * 4` bytes.
    pub raw: Vec<u8>,
    /// Time since the recorder was created.
    pub timestamp: Duration,
}

impl Frame {
    pub fn new(width: u32, height: u32, raw: Vec<u8>) -> Self {
        Self {
            width,
            height,
            raw,
            timestamp: Duration::ZERO,
        }
    }

    pub fn new_at(width: u32, height: u32, raw: Vec<u8>, timestamp: Duration) -> Self {
        Self {
            width,
            height,
            raw,
            timestamp,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct RecorderWaker {
    parking: Mutex<bool>,
    condvar: Condvar,
}

impl RecorderWaker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            parking: Mutex::new(true),
            condvar: Condvar::new(),
        }
    }
    #[allow(dead_code)]
    pub fn wake(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        *parking = false;
        self.condvar.notify_one();

        Ok(())
    }
    #[allow(dead_code)]
    pub fn sleep(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        *parking = true;

        Ok(())
    }
    #[allow(dead_code)]
    pub fn wait(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        while *parking {
            parking = self.condvar.wait(parking)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct VideoRecorder {
    impl_video_recorder: ImplVideoRecorder,
}

impl VideoRecorder {
    pub(crate) fn new(impl_video_recorder: ImplVideoRecorder) -> VideoRecorder {
        VideoRecorder {
            impl_video_recorder,
        }
    }
}

impl VideoRecorder {
    pub fn start(&self) -> XCapResult<()> {
        self.impl_video_recorder.start()
    }
    pub fn stop(&self) -> XCapResult<()> {
        self.impl_video_recorder.stop()
    }
}
