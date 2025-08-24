mod error;
mod monitor;
mod video_recorder;
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

#[cfg(target_os = "android")]
#[path = "android/mod.rs"]
mod platform;

pub use image;

pub use error::{XCapError, XCapResult};
pub use monitor::Monitor;
pub use window::Window;

pub use video_recorder::Frame;
pub use video_recorder::VideoRecorder;
