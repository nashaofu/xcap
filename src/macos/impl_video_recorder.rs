#![allow(unused)]

use crate::{video_recorder::Frame, XCapResult};

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {}

impl ImplVideoRecorder {
    pub fn new() -> XCapResult<Self> {
        unimplemented!()
    }

    pub fn on_frame<F>(&self, on_frame: F) -> XCapResult<()>
    where
        F: Fn(Frame) -> XCapResult<()> + Send + 'static,
    {
        unimplemented!()
    }
    pub fn start(&self) -> XCapResult<()> {
        unimplemented!()
    }
    pub fn stop(&self) -> XCapResult<()> {
        unimplemented!()
    }
}
