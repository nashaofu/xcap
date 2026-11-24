use std::{
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(any(target_os = "windows", test))]
use std::sync::{Arc, atomic::AtomicUsize};

use crate::{HdrImage, XCapResult, platform::impl_video_recorder::ImplVideoRecorder};

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

/// Which range a video recorder should deliver.
///
/// [`Hdr`](Self::Hdr) is a request, not a guarantee: the display has to be
/// composited in HDR mode and the platform has to expose extended-range frames.
/// When either is missing the recorder delivers ordinary SDR frames, so callers
/// must look at each [`Frame`] instead of assuming what they asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DynamicRange {
    #[default]
    Sdr,
    Hdr,
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
    /// Which frame size to deliver. [`OutputSize::Logical`] is a macOS-only
    /// best-effort hint; DXGI has no equivalent and always delivers
    /// [`OutputSize::Native`] frames, so callers must keep their own downscale
    /// as a fallback on every platform.
    pub output_size: OutputSize,
    /// Ask for extended-range frames. See [`DynamicRange`].
    pub dynamic_range: DynamicRange,
}

impl Default for VideoRecorderOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            region: None,
            captures_cursor: true,
            excludes_own_windows: true,
            output_size: OutputSize::Native,
            dynamic_range: DynamicRange::Sdr,
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

    /// Ask for frames at the source rectangle's logical size. macOS only;
    /// other platforms ignore it and keep delivering native-resolution frames.
    pub fn output_size(mut self, output_size: OutputSize) -> Self {
        self.output_size = output_size;
        self
    }

    pub fn dynamic_range(mut self, dynamic_range: DynamicRange) -> Self {
        self.dynamic_range = dynamic_range;
        self
    }
}

#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA pixels, `width * height * 4` bytes. Empty for an HDR frame.
    pub raw: Vec<u8>,
    /// The same frame as linear scRGB half-float samples, when the recorder
    /// actually delivered extended-range frames.
    pub hdr: Option<HdrImage>,
    /// Time since the recorder was created.
    pub timestamp: Duration,
    /// Released when the receiver drops the frame, including decode failures.
    #[cfg(any(target_os = "windows", test))]
    queue_permit: Option<QueuePermit>,
}

impl Clone for Frame {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            raw: self.raw.clone(),
            hdr: self.hdr.clone(),
            timestamp: self.timestamp,
            #[cfg(any(target_os = "windows", test))]
            queue_permit: self.queue_permit.clone(),
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug)]
pub(crate) struct QueuePermit {
    depth: Arc<AtomicUsize>,
}

#[cfg(any(target_os = "windows", test))]
impl QueuePermit {
    pub(crate) fn acquire(depth: Arc<AtomicUsize>, capacity: usize) -> Option<Self> {
        let mut current = depth.load(Ordering::Relaxed);
        loop {
            if current >= capacity {
                return None;
            }
            match depth.compare_exchange_weak(
                current,
                current + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(Self { depth }),
                Err(actual) => current = actual,
            }
        }
    }
}

#[cfg(any(target_os = "windows", test))]
impl Clone for QueuePermit {
    fn clone(&self) -> Self {
        self.depth.fetch_add(1, Ordering::Relaxed);
        Self {
            depth: self.depth.clone(),
        }
    }
}

#[cfg(any(target_os = "windows", test))]
impl Drop for QueuePermit {
    fn drop(&mut self) {
        self.depth.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Frame {
    pub fn new(width: u32, height: u32, raw: Vec<u8>) -> Self {
        Self {
            width,
            height,
            raw,
            hdr: None,
            timestamp: Duration::ZERO,
            #[cfg(any(target_os = "windows", test))]
            queue_permit: None,
        }
    }

    pub fn new_at(width: u32, height: u32, raw: Vec<u8>, timestamp: Duration) -> Self {
        Self {
            width,
            height,
            raw,
            hdr: None,
            timestamp,
            #[cfg(any(target_os = "windows", test))]
            queue_permit: None,
        }
    }

    pub fn new_hdr(width: u32, height: u32, hdr: HdrImage, timestamp: Duration) -> Self {
        Self {
            width,
            height,
            raw: Vec::new(),
            hdr: Some(hdr),
            timestamp,
            #[cfg(any(target_os = "windows", test))]
            queue_permit: None,
        }
    }

    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn with_queue_permit(mut self, permit: QueuePermit) -> Self {
        self.queue_permit = Some(permit);
        self
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct RecorderWaker {
    parking: Mutex<bool>,
    stopped: AtomicBool,
    condvar: Condvar,
}

impl RecorderWaker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            parking: Mutex::new(true),
            stopped: AtomicBool::new(false),
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
    pub fn shutdown(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        *parking = false;
        self.stopped.store(true, Ordering::Release);
        self.condvar.notify_all();

        Ok(())
    }
    #[allow(dead_code)]
    pub fn wait(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        while *parking && !self.stopped.load(Ordering::Acquire) {
            parking = self.condvar.wait(parking)?;
        }

        Ok(())
    }
    #[allow(dead_code)]
    pub fn wait_active(&self) -> XCapResult<bool> {
        self.wait()?;
        Ok(!self.stopped.load(Ordering::Acquire))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_permits_track_queued_and_cloned_frames() {
        let depth = Arc::new(AtomicUsize::new(0));
        let first = QueuePermit::acquire(depth.clone(), 2).unwrap();
        let second = QueuePermit::acquire(depth.clone(), 2).unwrap();
        assert!(QueuePermit::acquire(depth.clone(), 2).is_none());
        let cloned = second.clone();
        drop(second);
        assert_eq!(depth.load(Ordering::Relaxed), 2);
        drop(cloned);
        assert_eq!(depth.load(Ordering::Relaxed), 1);
        drop(first);
        assert_eq!(depth.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn receiving_a_frame_releases_its_queue_permit() {
        let depth = Arc::new(AtomicUsize::new(0));
        let permit = QueuePermit::acquire(depth.clone(), 1).unwrap();
        let frame = Frame::new(1, 1, vec![0; 4]).with_queue_permit(permit);
        assert_eq!(depth.load(Ordering::Relaxed), 1);
        drop(frame);
        assert_eq!(depth.load(Ordering::Relaxed), 0);
    }
}
