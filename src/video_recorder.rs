#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};

use crate::{XCapResult, platform::impl_video_recorder::ImplVideoRecorder};

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub raw: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, raw: Vec<u8>) -> Self {
        Self { width, height, raw }
    }
}

#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
#[derive(Debug)]
struct RecorderState {
    parked: bool,
    terminated: bool,
}

#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
#[derive(Debug)]
pub(crate) struct RecorderWaker {
    state: Mutex<RecorderState>,
    condvar: Condvar,
}

#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
impl RecorderWaker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RecorderState {
                parked: true,
                terminated: false,
            }),
            condvar: Condvar::new(),
        }
    }
    pub fn wake(&self) -> XCapResult<()> {
        let mut state = self.state.lock()?;
        if state.terminated {
            return Err(crate::XCapError::new("Recorder worker has exited"));
        }
        state.parked = false;
        self.condvar.notify_one();

        Ok(())
    }
    pub fn sleep(&self) -> XCapResult<()> {
        let mut state = self.state.lock()?;
        state.parked = true;

        Ok(())
    }
    pub fn wait(&self) -> XCapResult<bool> {
        let mut state = self.state.lock()?;
        while state.parked && !state.terminated {
            state = self.condvar.wait(state)?;
        }

        Ok(!state.terminated)
    }

    pub fn terminate(&self) {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        state.terminated = true;
        self.condvar.notify_all();
    }
}

#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
#[derive(Debug)]
pub(crate) struct RecorderWorker {
    pub waker: Arc<RecorderWaker>,
    pub handle: Option<JoinHandle<()>>,
}

#[cfg(any(
    all(target_os = "linux", not(target_env = "ohos")),
    all(target_os = "windows", not(feature = "wgc"))
))]
impl Drop for RecorderWorker {
    fn drop(&mut self) {
        self.waker.terminate();
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                log::error!("Recorder worker panicked");
            }
        }
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
    /// Start or resume capture.
    pub fn start(&self) -> XCapResult<()> {
        self.impl_video_recorder.start()
    }
    /// Pause capture. Frames already queued in the receiver remain available.
    pub fn stop(&self) -> XCapResult<()> {
        self.impl_video_recorder.stop()
    }
}
