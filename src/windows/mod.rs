mod capture;
#[cfg(not(feature = "wgc"))]
mod gdi;
mod utils;
#[cfg(feature = "wgc")]
mod wgc;

pub mod impl_monitor;
pub mod impl_video_recorder;
pub mod impl_window;
