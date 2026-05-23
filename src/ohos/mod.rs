// HarmonyOS (OpenHarmony) platform implementation.
// Screen capture in this module uses API 14 PixelMap capture and expects
// `ohos.permission.CUSTOM_SCREEN_CAPTURE` to be granted by the app layer.

mod capture;
mod ffi;
pub mod impl_monitor;
pub mod impl_video_recorder;
pub mod impl_window;

// Keep symbol export layout consistent with other platform modules.
#[allow(unused_imports)]
pub(crate) use impl_monitor::ImplMonitor;
