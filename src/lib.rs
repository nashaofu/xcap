mod capturer;
pub use capturer::Capturer;

#[cfg(target_os = "macos")]
mod darwin;
#[cfg(target_os = "macos")]
pub use darwin::*;

#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "windows")]
pub use win32::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
