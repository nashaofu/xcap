use std::{
    sync::{Arc, Condvar, Mutex},
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

/// Thread-safe pool of reusable RGBA frame byte buffers.
///
/// Callers such as scroll capture can pass a shared pool into
/// [`VideoRecorderOptions::buffer_pool`] and call [`Self::recycle`] once a
/// captured frame buffer is no longer needed, avoiding repeated large heap
/// allocations and zero-initialization on every frame.
#[derive(Clone, Debug)]
pub struct FrameBufferPool {
    buffers: Arc<Mutex<Vec<Vec<u8>>>>,
    max_buffers: usize,
}

impl FrameBufferPool {
    pub fn new(max_buffers: usize) -> Self {
        Self {
            buffers: Arc::new(Mutex::new(Vec::with_capacity(max_buffers))),
            max_buffers,
        }
    }

    /// Take a cleared buffer with at least `len` bytes of capacity, or
    /// allocate a fresh `Vec::with_capacity(len)`.
    pub fn take(&self, len: usize) -> Vec<u8> {
        let mut guard = self.buffers.lock().unwrap_or_else(|p| p.into_inner());
        while let Some(mut buf) = guard.pop() {
            let cap = buf.capacity();
            if cap >= len && cap <= len.saturating_mul(2) {
                buf.clear();
                return buf;
            }
        }
        drop(guard);
        Vec::with_capacity(len)
    }

    /// Return a consumed buffer to the pool if space remains.
    pub fn recycle(&self, mut buf: Vec<u8>) {
        if self.max_buffers == 0 || buf.capacity() == 0 {
            return;
        }
        let mut guard = self.buffers.lock().unwrap_or_else(|p| p.into_inner());
        if guard.len() < self.max_buffers {
            buf.clear();
            guard.push(buf);
        }
    }
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
    /// Override the internal platform-to-caller sync channel depth (`1..=16`).
    pub channel_capacity: Option<usize>,
    /// Optional buffer pool reused by the platform capture callback.
    pub buffer_pool: Option<FrameBufferPool>,
}

impl Default for VideoRecorderOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            region: None,
            captures_cursor: true,
            excludes_own_windows: true,
            output_size: OutputSize::Native,
            channel_capacity: None,
            buffer_pool: None,
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

    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = Some(capacity.clamp(1, 16));
        self
    }

    pub fn buffer_pool(mut self, pool: FrameBufferPool) -> Self {
        self.buffer_pool = Some(pool);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_buffer_pool_reuses_matching_buffers_and_caps_idle_count() {
        let pool = FrameBufferPool::new(2);
        let a = Vec::with_capacity(128);
        let b = Vec::with_capacity(128);
        let c = Vec::with_capacity(128);
        pool.recycle(a);
        pool.recycle(b);
        pool.recycle(c);

        let reused1 = pool.take(128);
        let reused2 = pool.take(128);
        assert!(reused1.capacity() >= 128 && reused2.capacity() >= 128);
        assert!(reused1.as_ptr() != reused2.as_ptr());

        // Returning mismatched capacities discards buffers instead of handing back wrong sizes.
        pool.recycle(vec![1u8; 64]);
        let larger = pool.take(128);
        assert!(larger.capacity() >= 128);

        pool.recycle(vec![0u8; 512]);
        let smaller = pool.take(128);
        assert!(smaller.capacity() <= 256);
    }
}
