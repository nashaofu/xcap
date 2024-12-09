mod error;
mod monitor;
mod window;

#[cfg(target_os = "macos")]
#[path = "macos/mod.rs"]
mod platform;

#[cfg(target_os = "windows")]
#[path = "windows/mod.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "linux/mod.rs"]
mod platform;

pub use image;

pub use error::{XCapError, XCapResult};
pub use monitor::Monitor;
pub use window::Window;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub buffer: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, buffer: Vec<u8>) -> Self {
        Self {
            width,
            height,
            buffer,
        }
    }
}
