use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

use crate::{XCapResult, platform::impl_video_recorder::ImplVideoRecorder};

/// Upper bound accepted for an explicit video recorder frame-rate cap.
pub(crate) const MAX_FPS: u32 = 360;

/// A rectangle in monitor units, the same space as [`crate::Monitor::capture_region`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Which frame size a video recorder should deliver.
///
/// A capture region is expressed in monitor units (display points on macOS),
/// while frames are always pixels. On a Retina display those differ by the
/// monitor's scale factor, which is a platform detail callers do not know.
/// This enum lets a caller state the intent instead of having to compute a
/// pixel size itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputSize {
    /// Physical pixels: the source rectangle multiplied by the scale factor.
    /// The default, and the only size DXGI can deliver.
    #[default]
    Native,
    /// The source rectangle's size in monitor units. On macOS
    /// ScreenCaptureKit downscales in the compositor before the sample buffer
    /// reaches the callback, so a 2x display delivers a quarter of the pixels
    /// and the height is simply the region height — no aspect-ratio maths.
    /// macOS only: other platforms deliver `Native` frames instead, so callers
    /// must keep their own downscale as a fallback.
    Logical,
}

/// Options for [`crate::Monitor::video_recorder_with_options`].
#[derive(Debug, Clone)]
pub struct VideoRecorderOptions {
    /// Upper bound on the delivered frame rate.
    ///
    /// `0` requests the monitor's native refresh rate (no artificial 60 fps
    /// cap); `1..=360` caps delivery to at most that many frames per second
    /// (values above `360` clamp to `360`).
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
    /// Which frame size to deliver. [`OutputSize::Logical`] is a macOS-only
    /// best-effort hint; DXGI has no equivalent and always delivers
    /// [`OutputSize::Native`] frames, so callers must keep their own downscale
    /// as a fallback on every platform.
    pub output_size: OutputSize,
}

impl Default for VideoRecorderOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            region: None,
            captures_cursor: true,
            excludes_own_windows: true,
            output_size: OutputSize::Native,
        }
    }
}

impl VideoRecorderOptions {
    /// Set the frame-rate cap (`0` for the monitor's native refresh rate, or
    /// `1..=360` frames per second; values above `360` clamp to `360`).
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps.min(MAX_FPS);
        self
    }

    /// Deliver frames at the monitor's native refresh rate without an
    /// artificial frame-rate cap.
    pub fn native_fps(mut self) -> Self {
        self.fps = 0;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn resolved_frame_interval(&self) -> Option<Duration> {
        if self.fps == 0 {
            None
        } else {
            Some(Duration::from_secs_f64(
                1.0 / self.fps.clamp(1, MAX_FPS) as f64,
            ))
        }
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

    /// Ask for frames at the source rectangle's logical size. macOS only;
    /// other platforms ignore it and keep delivering native-resolution frames.
    pub fn output_size(mut self, output_size: OutputSize) -> Self {
        self.output_size = output_size;
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

#[cfg(any(target_os = "windows", test))]
pub(crate) fn throttle_tolerance(interval: Duration) -> Duration {
    Duration::from_millis(1).min(interval / 4)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_options_support_native_and_high_refresh_rates() {
        let default = VideoRecorderOptions::default();
        assert_eq!(default.fps, 60);
        assert!(default.resolved_frame_interval().is_some());

        let zero = VideoRecorderOptions::default().fps(0);
        assert_eq!(zero.fps, 0);
        assert!(zero.resolved_frame_interval().is_none());

        let native = VideoRecorderOptions::default().native_fps();
        assert_eq!(native.fps, 0);
        assert!(native.resolved_frame_interval().is_none());

        let hz120 = VideoRecorderOptions::default().fps(120);
        assert_eq!(hz120.fps, 120);
        let interval_240 = VideoRecorderOptions::default()
            .fps(240)
            .resolved_frame_interval()
            .unwrap();
        assert!(interval_240 < Duration::from_millis(5));

        let clamped = VideoRecorderOptions::default().fps(1000);
        assert_eq!(clamped.fps, MAX_FPS);
    }

    #[test]
    fn throttle_tolerance_scales_with_high_refresh_intervals() {
        let hz60 = Duration::from_secs_f64(1.0 / 60.0);
        assert_eq!(throttle_tolerance(hz60), Duration::from_millis(1));

        let hz240 = Duration::from_secs_f64(1.0 / 240.0);
        assert!(throttle_tolerance(hz240) <= hz240 / 4);
    }
}
