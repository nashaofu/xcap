use std::sync::mpsc::Receiver;

use crate::{XCapResult, video_recorder::Frame};

use super::{
    impl_monitor::ImplMonitor, utils::wayland_detect, wayland_video_recorder::WaylandVideoRecorder,
    xorg_video_recorder::XorgVideoRecorder,
};

#[derive(Debug, Clone)]
pub enum ImplVideoRecorder {
    Xorg(XorgVideoRecorder),
    Wayland(WaylandVideoRecorder),
}

impl ImplVideoRecorder {
    pub fn new(monitor: ImplMonitor) -> XCapResult<(Self, Receiver<Frame>)> {
        if wayland_detect() {
            let (recorder, receiver) = WaylandVideoRecorder::new(monitor)?;
            Ok((ImplVideoRecorder::Wayland(recorder), receiver))
        } else {
            let (recorder, receiver) = XorgVideoRecorder::new(monitor)?;
            Ok((ImplVideoRecorder::Xorg(recorder), receiver))
        }
    }

    pub fn start(&self) -> XCapResult<()> {
        match self {
            ImplVideoRecorder::Xorg(recorder) => recorder.start(),
            ImplVideoRecorder::Wayland(recorder) => recorder.start(),
        }
    }

    pub fn stop(&self) -> XCapResult<()> {
        match self {
            ImplVideoRecorder::Xorg(recorder) => recorder.stop(),
            ImplVideoRecorder::Wayland(recorder) => recorder.stop(),
        }
    }
}
