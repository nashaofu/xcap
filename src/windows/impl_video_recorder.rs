#[cfg(feature = "wgc")]
pub(crate) use super::wgc_video_recorder::ImplVideoRecorder;

#[cfg(not(feature = "wgc"))]
pub(crate) use super::dxgi_video_recorder::ImplVideoRecorder;
