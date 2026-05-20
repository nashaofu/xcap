//! Persistent video recorder for OHOS using `OH_AVScreenCapture`.
//!
//! `ImplVideoRecorder` wraps a long-lived capture session.  Frames are
//! delivered to the caller via a bounded `std::sync::mpsc` channel.
//!
//! ## Lifecycle
//!
//! ```text
//! ImplVideoRecorder::new()   — creates & configures the session
//!   .start()                 — calls StartScreenCapture; frames flow into rx
//!   .stop()                  — calls StopScreenCapture; frames stop flowing
//!   drop()                   — calls Release; session is destroyed
//! ```
//!
//! `start()` / `stop()` may be called repeatedly.

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::{self, Receiver, SyncSender}};

use crate::{
    error::{XCapError, XCapResult},
    video_recorder::Frame,
};

use super::ffi;

// ── Shared state between ImplVideoRecorder and the FFI callback ───────────────

struct RecorderShared {
    /// The capture session handle; valid until `OH_AVScreenCapture_Release`.
    capture: *mut ffi::OH_AVScreenCapture,
    width: u32,
    height: u32,
    /// Channel endpoint used to push frames to the caller.
    tx: SyncSender<Frame>,
    /// `true` while the caller wants frames delivered to `tx`.
    frame_running: AtomicBool,
    /// `true` while `StartScreenCapture` has been called and not yet stopped.
    capture_active: AtomicBool,
}

// SAFETY: `OH_AVScreenCapture` callbacks may arrive on any native thread.
// All shared state is accessed through atomics or the `SyncSender` which is
// `Send + Sync`.
unsafe impl Send for RecorderShared {}
unsafe impl Sync for RecorderShared {}

impl Drop for RecorderShared {
    fn drop(&mut self) {
        // Stop the underlying capture if it is still running, then release.
        unsafe {
            if self.capture_active.load(Ordering::Acquire) {
                ffi::OH_AVScreenCapture_StopScreenCapture(self.capture);
            }
            ffi::OH_AVScreenCapture_Release(self.capture);
        }
    }
}

// ── Public struct ─────────────────────────────────────────────────────────────

/// OHOS screen-capture video recorder.
///
/// Cloning this value shares the same underlying capture session.
#[derive(Clone)]
pub(crate) struct ImplVideoRecorder {
    shared: Arc<RecorderShared>,
}

impl std::fmt::Debug for ImplVideoRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImplVideoRecorder")
            .field("width", &self.shared.width)
            .field("height", &self.shared.height)
            .finish()
    }
}

impl ImplVideoRecorder {
    // ── Constructor ───────────────────────────────────────────────────────────

    /// Create a new recorder for `display_id` without starting capture.
    ///
    /// Returns `(recorder, rx)` where `rx` is the channel from which callers
    /// receive `Frame` values after calling `start()`.
    pub fn new(display_id: u64, width: u32, height: u32) -> XCapResult<(Self, Receiver<Frame>)> {
        let capture = unsafe { ffi::OH_AVScreenCapture_Create() };
        if capture.is_null() {
            return Err(XCapError::new("OH_AVScreenCapture_Create returned null"));
        }

        // Build a zero-initialised config, then fill the video section.
        // SAFETY: All-zero bytes are valid defaults ("unset") for this C struct.
        let mut config: ffi::OH_AVScreenCaptureConfig = unsafe { core::mem::zeroed() };
        config.captureMode = ffi::OH_CaptureMode::SpecifiedScreen;
        config.dataType = ffi::OH_DataType::OriginalStream;
        config.videoInfo.videoCapInfo.displayId = display_id;
        config.videoInfo.videoCapInfo.videoFrameWidth = width as i32;
        config.videoInfo.videoCapInfo.videoFrameHeight = height as i32;
        config.videoInfo.videoCapInfo.videoSource = ffi::OH_VideoSourceType::Rgba;

        let err = unsafe { ffi::OH_AVScreenCapture_Init(capture, config) };
        if err != ffi::OH_AVSCREEN_CAPTURE_ErrCode::Ok {
            unsafe { ffi::OH_AVScreenCapture_Release(capture) };
            return Err(XCapError::new(format!(
                "OH_AVScreenCapture_Init failed: {:?}",
                err
            )));
        }

        let (tx, rx) = mpsc::sync_channel::<Frame>(10);

        let shared = Arc::new(RecorderShared {
            capture,
            width,
            height,
            tx,
            frame_running: AtomicBool::new(false),
            capture_active: AtomicBool::new(false),
        });

        // We pass a raw pointer to the `RecorderShared` data (i.e., the
        // allocation managed by the Arc) as the callback `user_data`.
        //
        // Safety contract:
        //   - The raw pointer is valid for as long as the `Arc<RecorderShared>`
        //     is alive, which is guaranteed because:
        //       (a) `ImplVideoRecorder` holds one Arc clone, and
        //       (b) `RecorderShared::drop` calls `Release` *before* the memory
        //           is freed, ensuring no further callbacks fire.
        //   - After `OH_AVScreenCapture_Release` returns, the OHOS runtime
        //     guarantees no further callbacks are invoked.
        //
        // We intentionally do NOT use `Arc::into_raw` here, because that would
        // make the Arc leak unless explicitly reclaimed.  The borrowed raw
        // pointer is safe because `Arc` keeps the allocation alive.
        let shared_raw = Arc::as_ptr(&shared) as *mut c_void;

        unsafe {
            ffi::OH_AVScreenCapture_SetStateCallback(
                capture,
                on_state_change,
                core::ptr::null_mut(),
            );
            ffi::OH_AVScreenCapture_SetErrorCallback(
                capture,
                on_error,
                core::ptr::null_mut(),
            );
            ffi::OH_AVScreenCapture_SetDataCallback(capture, on_buffer_recorder, shared_raw);
        }

        Ok((ImplVideoRecorder { shared }, rx))
    }

    // ── Control ───────────────────────────────────────────────────────────────

    /// Begin (or resume) frame delivery.
    ///
    /// The first call starts the underlying `OH_AVScreenCapture` session.
    /// Subsequent calls after `stop()` restart it.
    pub fn start(&self) -> XCapResult<()> {
        // Signal the callback to start delivering frames.
        self.shared.frame_running.store(true, Ordering::Release);

        if !self.shared.capture_active.load(Ordering::Acquire) {
            let err = unsafe {
                ffi::OH_AVScreenCapture_StartScreenCapture(self.shared.capture)
            };
            if err != ffi::OH_AVSCREEN_CAPTURE_ErrCode::Ok {
                self.shared.frame_running.store(false, Ordering::Release);
                return Err(XCapError::new(format!(
                    "OH_AVScreenCapture_StartScreenCapture failed: {:?}",
                    err
                )));
            }
            self.shared.capture_active.store(true, Ordering::Release);
        }

        Ok(())
    }

    /// Stop frame delivery and the underlying capture session.
    ///
    /// After this call the receiver will not receive any further frames.
    /// `start()` can be called again to restart capture.
    pub fn stop(&self) -> XCapResult<()> {
        // Signal the callback to stop delivering frames first to avoid a
        // race where one last frame arrives after StopScreenCapture returns.
        self.shared.frame_running.store(false, Ordering::Release);

        if self.shared.capture_active.swap(false, Ordering::AcqRel) {
            let err = unsafe {
                ffi::OH_AVScreenCapture_StopScreenCapture(self.shared.capture)
            };
            if err != ffi::OH_AVSCREEN_CAPTURE_ErrCode::Ok {
                return Err(XCapError::new(format!(
                    "OH_AVScreenCapture_StopScreenCapture failed: {:?}",
                    err
                )));
            }
        }

        Ok(())
    }
}

// ── FFI callbacks ─────────────────────────────────────────────────────────────

unsafe extern "C" fn on_state_change(
    _capture: *mut ffi::OH_AVScreenCapture,
    state: ffi::OH_AVScreenCaptureStateCode,
    _user_data: *mut c_void,
) {
    log::debug!("OH_AVScreenCapture state: {:?}", state);
}

unsafe extern "C" fn on_error(
    _capture: *mut ffi::OH_AVScreenCapture,
    error_code: i32,
    _user_data: *mut c_void,
) {
    log::error!("OH_AVScreenCapture error {}", error_code);
}

/// Invoked on a native OHOS thread when a new buffer is available.
unsafe extern "C" fn on_buffer_recorder(
    _capture: *mut ffi::OH_AVScreenCapture,
    buffer: *mut ffi::OH_AVBuffer,
    buffer_type: ffi::OH_AVScreenCaptureBufferType,
    _timestamp: i64,
    user_data: *mut c_void,
) {
    // Only process video (RGBA) buffers.
    if buffer_type != ffi::OH_AVScreenCaptureBufferType::Video {
        return;
    }

    // SAFETY: `user_data` is `Arc::as_ptr(&shared)` cast to `*mut c_void`.
    // The `Arc<RecorderShared>` outlives any callback because `RecorderShared::drop`
    // calls `OH_AVScreenCapture_Release` (which waits for in-flight callbacks)
    // before the allocation is freed.
    let shared = &*(user_data as *const RecorderShared);

    // Skip if the caller has not started (or has paused) frame delivery.
    if !shared.frame_running.load(Ordering::Acquire) {
        return;
    }

    let addr = ffi::OH_AVBuffer_GetAddr(buffer);
    if addr.is_null() {
        return;
    }

    let capacity = ffi::OH_AVBuffer_GetCapacity(buffer);
    if capacity <= 0 {
        return;
    }

    // SAFETY: `addr` points to `capacity` valid, CPU-accessible bytes for the
    // duration of this callback invocation.
    let data = std::slice::from_raw_parts(addr, capacity as usize).to_vec();
    let frame = Frame::new(shared.width, shared.height, data);

    if let Err(e) = shared.tx.try_send(frame) {
        log::warn!("xcap: OHOS frame dropped: {}", e);
    }
}
