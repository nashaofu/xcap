//! Raw FFI bindings to OHOS NDK C APIs used for screen capture:
//!
//! - `libnative_display_manager.so` — Display enumeration and info
//! - `libnative_avscreen_capture.so` — Screen capture / recording
//! - `libnative_media_core.so` — `OH_AVBuffer` helpers
//! - `libnative_buffer.so` — `OH_NativeBuffer` helpers
//!
//! All function calls are `unsafe`. Structs use `#[repr(C)]` to match the
//! C ABI of the OHOS NDK.

#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::upper_case_acronyms)]

use std::ffi::c_char;
use std::os::raw::c_void;

// ── Display Manager ───────────────────────────────────────────────────────────

/// Maximum number of characters in a display name (excluding NUL terminator).
pub const OH_DISPLAY_NAME_LENGTH: usize = 32;

/// Error codes returned by `OH_NativeDisplayManager_*` functions.
///
/// Source: `oh_display_info.h` (Since: 12)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplayManager_ErrorCode {
    Ok = 0,
    NoPermission = 201,
    NotSystemApp = 202,
    InvalidParam = 401,
    DeviceNotSupported = 801,
    InvalidScreen = 1400001,
    InvalidCall = 1400002,
    SystemAbnormal = 1400003,
}

/// Clockwise rotation angle of a display.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplayManager_Rotation {
    Rotation0 = 0,
    Rotation90 = 1,
    Rotation180 = 2,
    Rotation270 = 3,
}

/// Orientation of a display.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplayManager_Orientation {
    Portrait = 0,
    Landscape = 1,
    PortraitInverted = 2,
    LandscapeInverted = 3,
    Unknown = 4,
}

/// Power/lifecycle state of a display.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplayManager_DisplayState {
    Unknown = 0,
    Off = 1,
    On = 2,
    Doze = 3,
    DozeSuspend = 4,
    VR = 5,
    OnSuspend = 6,
}

/// Inner HDR format list of a display (pointer-managed by the NDK).
#[repr(C)]
pub struct NativeDisplayManager_DisplayHdrFormat {
    pub hdrFormatLength: u32,
    pub hdrFormats: *mut u32,
}

/// Inner colour-space list of a display (pointer-managed by the NDK).
#[repr(C)]
pub struct NativeDisplayManager_DisplayColorSpace {
    pub colorSpaceLength: u32,
    pub colorSpaces: *mut u32,
}

/// Per-display information returned by the NDK (Since: 14).
///
/// Field layout must match `NativeDisplayManager_DisplayInfo` in `oh_display_info.h`.
#[repr(C)]
pub struct NativeDisplayManager_DisplayInfo {
    /// Unique display ID (non-negative).
    pub id: u32,
    /// Display name string (NUL-terminated, max `OH_DISPLAY_NAME_LENGTH` chars).
    pub name: [c_char; OH_DISPLAY_NAME_LENGTH + 1],
    /// Whether the display is active.
    pub isAlive: bool,
    /// Logical width in px.
    pub width: i32,
    /// Logical height in px.
    pub height: i32,
    /// Physical width in px.
    pub physicalWidth: i32,
    /// Physical height in px.
    pub physicalHeight: i32,
    /// Refresh rate in Hz.
    pub refreshRate: u32,
    /// Available width (2-in-1 devices only), in px.
    pub availableWidth: u32,
    /// Available height (2-in-1 devices only), in px.
    pub availableHeight: u32,
    /// Physical pixels per inch (DPI).
    pub densityDPI: f32,
    /// Logical pixel density (scale factor).
    pub densityPixels: f32,
    /// Font scaling factor.
    pub scaledDensity: f32,
    /// Exact physical pixels per inch in the X dimension.
    pub xDPI: f32,
    /// Exact physical pixels per inch in the Y dimension.
    pub yDPI: f32,
    /// Clockwise rotation angle of the display.
    pub rotation: NativeDisplayManager_Rotation,
    /// Power state of the display.
    pub state: NativeDisplayManager_DisplayState,
    /// Orientation of the display.
    pub orientation: NativeDisplayManager_Orientation,
    /// HDR formats supported by the display (NDK-managed).
    pub hdrFormat: *mut NativeDisplayManager_DisplayHdrFormat,
    /// Colour spaces supported by the display (NDK-managed).
    pub colorSpace: *mut NativeDisplayManager_DisplayColorSpace,
}

/// Container for information about all displays on the device.
#[repr(C)]
pub struct NativeDisplayManager_DisplaysInfo {
    /// Number of entries in `displaysInfo`.
    pub displaysLength: u32,
    /// Array of per-display info structs (NDK-managed).
    pub displaysInfo: *mut NativeDisplayManager_DisplayInfo,
}

// ── AVScreenCapture ───────────────────────────────────────────────────────────

/// Opaque screen-capture session handle.
#[repr(C)]
pub struct OH_AVScreenCapture {
    _opaque: [u8; 0],
}

/// Opaque AV buffer handle (`native_media_core`).
#[repr(C)]
pub struct OH_AVBuffer {
    _opaque: [u8; 0],
}

/// Opaque native (GPU) buffer handle (`native_buffer`).
#[repr(C)]
pub struct OH_NativeBuffer {
    _opaque: [u8; 0],
}

/// Opaque PixelMap handle (`libpixelmap.so`).
#[repr(C)]
pub struct OH_PixelmapNative {
    _opaque: [u8; 0],
}

/// Opaque PixelMap image-info handle (`libpixelmap.so`).
#[repr(C)]
pub struct OH_Pixelmap_ImageInfo {
    _opaque: [u8; 0],
}

/// RGBA_8888 pixel format (value 3 in OHOS PixelMap API).
pub const PIXEL_FORMAT_RGBA_8888: i32 = 3;
/// BGRA_8888 pixel format (value 4 in OHOS PixelMap API).
pub const PIXEL_FORMAT_BGRA_8888: i32 = 4;
/// Success code returned by `OH_PixelmapNative_*` / `OH_PixelmapImageInfo_*` functions.
pub const IMAGE_SUCCESS: i32 = 0;

// -- Enums --

/// Screen capture mode.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_CaptureMode {
    HomeScreen = 0,
    SpecifiedScreen = 1,
    SpecifiedWindow = 2,
    Invalid = -1,
}

/// Data type of the capture stream.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_DataType {
    OriginalStream = 0,
    EncodedStream = 1,
    CaptureFile = 2,
    Invalid = -1,
}

/// Video source pixel format.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_VideoSourceType {
    Yuv = 0,
    Es = 1,
    Rgba = 2,
    Butt = 3,
}

/// Audio source type during screen capture.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_AudioCaptureSourceType {
    Invalid = -1,
    Default = 0,
    Mic = 1,
    AllPlayback = 2,
    AppPlayback = 3,
}

/// Audio codec format.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_AudioCodecFormat {
    AudioDefault = 0,
    AacLc = 3,
    Butt = 100,
}

/// Video codec format.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_VideoCodecFormat {
    VideoDefault = 0,
    H264 = 2,
    H265 = 4,
    Mpeg4 = 6,
    VP8 = 8,
    VP9 = 10,
    Butt = 100,
}

/// File container format.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_ContainerFormatType {
    M4A = 0,
    MP4 = 1,
}

/// Type of an AV buffer delivered to `OH_AVScreenCapture_OnBufferAvailable`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_AVScreenCaptureBufferType {
    Video = 0,
    AudioInner = 1,
    AudioMic = 2,
}

/// State codes reported by `OH_AVScreenCapture_OnStateChange`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_AVScreenCaptureStateCode {
    Started = 0,
    Canceled = 1,
    StoppedByUser = 2,
    InterruptedByOther = 3,
    StoppedByCall = 4,
    MicUnavailable = 5,
    MicMutedByUser = 6,
    MicUnmutedByUser = 7,
    EnterPrivateScene = 8,
    ExitPrivateScene = 9,
    StoppedByUserSwitches = 10,
}

/// Error codes returned by `OH_AVScreenCapture_*` functions.
///
/// `AV_SCREEN_CAPTURE_ERR_BASE = 0` → `OK = 0`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OH_AVSCREEN_CAPTURE_ErrCode {
    Ok = 0,
    NoMemory = 1,
    OperateNotPermit = 2,
    InvalidVal = 3,
    IO = 4,
    Timeout = 5,
    Unknown = 6,
    ServiceDied = 7,
    InvalidState = 8,
    Unsupport = 9,
}

// -- Configuration structs --

/// Audio capture parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OH_AudioCaptureInfo {
    pub audioSampleRate: i32,
    pub audioChannels: i32,
    pub audioSource: OH_AudioCaptureSourceType,
}

/// Audio encoding parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OH_AudioEncInfo {
    pub audioBitrate: i32,
    pub audioCodecformat: OH_AudioCodecFormat,
}

/// Combined audio information.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OH_AudioInfo {
    pub micCapInfo: OH_AudioCaptureInfo,
    pub innerCapInfo: OH_AudioCaptureInfo,
    pub audioEncInfo: OH_AudioEncInfo,
}

/// Video capture parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OH_VideoCaptureInfo {
    /// Display ID to capture (used when `captureMode == SpecifiedScreen`).
    pub displayId: u64,
    /// Mission ID list (used when `captureMode == SpecifiedWindow`).
    pub missionIDs: *mut i32,
    /// Length of `missionIDs`.
    pub missionIDsLen: i32,
    /// Capture width in px.
    pub videoFrameWidth: i32,
    /// Capture height in px.
    pub videoFrameHeight: i32,
    /// Pixel format of the video source.
    pub videoSource: OH_VideoSourceType,
}

/// Video encoding parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OH_VideoEncInfo {
    pub videoCodec: OH_VideoCodecFormat,
    pub videoBitrate: i32,
    pub videoFrameRate: i32,
}

/// Combined video information.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OH_VideoInfo {
    pub videoCapInfo: OH_VideoCaptureInfo,
    pub videoEncInfo: OH_VideoEncInfo,
}

/// Output file information (used when `dataType == CaptureFile`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OH_RecorderInfo {
    pub url: *const c_char,
    pub urlLen: i32,
    pub fileFormat: OH_ContainerFormatType,
}

/// Top-level configuration struct passed to `OH_AVScreenCapture_Init`.
#[repr(C)]
pub struct OH_AVScreenCaptureConfig {
    pub captureMode: OH_CaptureMode,
    pub dataType: OH_DataType,
    pub audioInfo: OH_AudioInfo,
    pub videoInfo: OH_VideoInfo,
    pub recorderInfo: OH_RecorderInfo,
}

// -- Callback function pointer types --

/// Called when the capture session state changes.
pub type OH_AVScreenCapture_OnStateChange = unsafe extern "C" fn(
    capture: *mut OH_AVScreenCapture,
    state_code: OH_AVScreenCaptureStateCode,
    user_data: *mut c_void,
);

/// Called when an error occurs in the capture session.
pub type OH_AVScreenCapture_OnError = unsafe extern "C" fn(
    capture: *mut OH_AVScreenCapture,
    error_code: i32,
    user_data: *mut c_void,
);

/// Called when an audio or video buffer is available.
pub type OH_AVScreenCapture_OnBufferAvailable = unsafe extern "C" fn(
    capture: *mut OH_AVScreenCapture,
    buffer: *mut OH_AVBuffer,
    buffer_type: OH_AVScreenCaptureBufferType,
    timestamp: i64,
    user_data: *mut c_void,
);

// ── Extern function declarations ──────────────────────────────────────────────

#[link(name = "native_display_manager")]
unsafe extern "C" {
    /// Obtain info for all connected displays.  Caller must free with
    /// `OH_NativeDisplayManager_DestroyAllDisplays`.
    pub fn OH_NativeDisplayManager_CreateAllDisplays(
        all_displays: *mut *mut NativeDisplayManager_DisplaysInfo,
    ) -> NativeDisplayManager_ErrorCode;

    /// Free display info obtained via `OH_NativeDisplayManager_CreateAllDisplays`.
    pub fn OH_NativeDisplayManager_DestroyAllDisplays(
        all_displays: *mut NativeDisplayManager_DisplaysInfo,
    );

    /// Obtain info for the primary display.  Caller must free with
    /// `OH_NativeDisplayManager_DestroyDisplay`.
    pub fn OH_NativeDisplayManager_CreatePrimaryDisplay(
        display_info: *mut *mut NativeDisplayManager_DisplayInfo,
    ) -> NativeDisplayManager_ErrorCode;

    /// Free display info obtained via `OH_NativeDisplayManager_CreatePrimaryDisplay`
    /// or `OH_NativeDisplayManager_CreateDisplayById`.
    pub fn OH_NativeDisplayManager_DestroyDisplay(
        display_info: *mut NativeDisplayManager_DisplayInfo,
    );

    /// Obtain the on-screen position (top-left corner) of a display.
    pub fn OH_NativeDisplayManager_GetDisplayPosition(
        display_id: u64,
        x: *mut i32,
        y: *mut i32,
    ) -> NativeDisplayManager_ErrorCode;

    /// Capture a single-frame screenshot of the specified display as a PixelMap.
    ///
    /// Requires `ohos.permission.CUSTOM_SCREEN_CAPTURE`. Since API 14.
    /// Library: `libnative_display_manager.so`.
    /// Returns 0 on success; non-zero error code otherwise (see `oh_display_info.h`).
    pub fn OH_NativeDisplayManager_CaptureScreenPixelmap(
        display_id: u32,
        pixel_map: *mut *mut OH_PixelmapNative,
    ) -> i32;
}

#[link(name = "native_avscreen_capture")]
unsafe extern "C" {
    /// Allocate a new `OH_AVScreenCapture` instance.
    pub fn OH_AVScreenCapture_Create() -> *mut OH_AVScreenCapture;

    /// Apply configuration to an `OH_AVScreenCapture` instance.
    pub fn OH_AVScreenCapture_Init(
        capture: *mut OH_AVScreenCapture,
        config: OH_AVScreenCaptureConfig,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Start capturing raw streams.
    pub fn OH_AVScreenCapture_StartScreenCapture(
        capture: *mut OH_AVScreenCapture,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Stop capturing raw streams.
    pub fn OH_AVScreenCapture_StopScreenCapture(
        capture: *mut OH_AVScreenCapture,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Destroy an `OH_AVScreenCapture` instance and free its resources.
    pub fn OH_AVScreenCapture_Release(
        capture: *mut OH_AVScreenCapture,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Register a state-change callback.  Must be called before `StartScreenCapture`.
    pub fn OH_AVScreenCapture_SetStateCallback(
        capture: *mut OH_AVScreenCapture,
        callback: OH_AVScreenCapture_OnStateChange,
        user_data: *mut c_void,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Register a data callback.  Must be called before `StartScreenCapture`.
    pub fn OH_AVScreenCapture_SetDataCallback(
        capture: *mut OH_AVScreenCapture,
        callback: OH_AVScreenCapture_OnBufferAvailable,
        user_data: *mut c_void,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Register an error callback.  Must be called before `StartScreenCapture`.
    pub fn OH_AVScreenCapture_SetErrorCallback(
        capture: *mut OH_AVScreenCapture,
        callback: OH_AVScreenCapture_OnError,
        user_data: *mut c_void,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;

    /// Polling API (API level 10+): acquire the next available video buffer.
    ///
    /// Returns `null` if no frame is ready.  Caller must call
    /// `OH_AVScreenCapture_ReleaseVideoBuffer` after processing.
    pub fn OH_AVScreenCapture_AcquireVideoBuffer(
        capture: *mut OH_AVScreenCapture,
        fence: *mut i32,
        timestamp: *mut i64,
        region: *mut OH_Rect,
    ) -> *mut OH_NativeBuffer;

    /// Release the buffer previously acquired by `OH_AVScreenCapture_AcquireVideoBuffer`.
    pub fn OH_AVScreenCapture_ReleaseVideoBuffer(
        capture: *mut OH_AVScreenCapture,
    ) -> OH_AVSCREEN_CAPTURE_ErrCode;
}

/// Screen region returned by `OH_AVScreenCapture_AcquireVideoBuffer`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OH_Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// `OH_NativeBuffer` layout / pixel-format description.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OH_NativeBuffer_Config {
    /// Width of the image in pixels.
    pub width: i32,
    /// Height of the image in pixels.
    pub height: i32,
    /// Pixel format (e.g. `NATIVEBUFFER_PIXEL_FMT_RGBA_8888 = 12`).
    pub format: i32,
    /// Buffer usage flags (`int32_t` per NDK `oh_native_buffer.h`).
    pub usage: i32,
    /// Row stride in pixels (may be > `width` due to hardware alignment).
    pub stride: i32,
}

#[link(name = "pixelmap")]
unsafe extern "C" {
    /// Allocate a new `OH_Pixelmap_ImageInfo` to be populated by `OH_PixelmapNative_GetImageInfo`.
    pub fn OH_PixelmapImageInfo_Create(info: *mut *mut OH_Pixelmap_ImageInfo) -> i32;

    /// Get the image width in pixels.
    pub fn OH_PixelmapImageInfo_GetWidth(info: *mut OH_Pixelmap_ImageInfo, width: *mut u32) -> i32;

    /// Get the image height in pixels.
    pub fn OH_PixelmapImageInfo_GetHeight(info: *mut OH_Pixelmap_ImageInfo, height: *mut u32) -> i32;

    /// Get the row stride in bytes (may be greater than `width × bytes_per_pixel` due to alignment).
    pub fn OH_PixelmapImageInfo_GetRowStride(info: *mut OH_Pixelmap_ImageInfo, row_stride: *mut u32) -> i32;

    /// Get the pixel format code (e.g. `PIXEL_FORMAT_RGBA_8888 = 3`, `PIXEL_FORMAT_BGRA_8888 = 4`).
    pub fn OH_PixelmapImageInfo_GetPixelFormat(info: *mut OH_Pixelmap_ImageInfo, pixel_format: *mut i32) -> i32;

    /// Free an `OH_Pixelmap_ImageInfo` obtained via `OH_PixelmapImageInfo_Create`.
    pub fn OH_PixelmapImageInfo_Release(info: *mut OH_Pixelmap_ImageInfo) -> i32;

    /// Populate `image_info` with metadata (width, height, stride, format) from `pixelmap`.
    pub fn OH_PixelmapNative_GetImageInfo(
        pixelmap: *mut OH_PixelmapNative,
        image_info: *mut OH_Pixelmap_ImageInfo,
    ) -> i32;

    /// Read all pixel data from `pixelmap` into `destination`.
    ///
    /// `*buffer_size` in: capacity (must be ≥ rowStride × height); out: bytes written.
    /// Data is in the pixelmap's native format (check `GetPixelFormat`).
    pub fn OH_PixelmapNative_ReadPixels(
        pixelmap: *mut OH_PixelmapNative,
        destination: *mut u8,
        buffer_size: *mut usize,
    ) -> i32;

    /// Release a `OH_PixelmapNative` and free its resources.
    pub fn OH_PixelmapNative_Release(pixelmap: *mut OH_PixelmapNative) -> i32;
}

#[link(name = "native_media_core")]
unsafe extern "C" {
    /// Return the CPU-accessible start address of the buffer's data.
    pub fn OH_AVBuffer_GetAddr(buffer: *mut OH_AVBuffer) -> *mut u8;

    /// Return the total byte capacity of the buffer.
    pub fn OH_AVBuffer_GetCapacity(buffer: *mut OH_AVBuffer) -> i32;

    /// Return the underlying `OH_NativeBuffer` (increments its reference count).
    pub fn OH_AVBuffer_GetNativeBuffer(buffer: *mut OH_AVBuffer) -> *mut OH_NativeBuffer;
}

#[link(name = "native_buffer")]
unsafe extern "C" {
    /// Decrement the reference count of `buffer`; frees it when count reaches 0.
    pub fn OH_NativeBuffer_Unreference(buffer: *mut OH_NativeBuffer) -> i32;

    /// Fill `config` with the layout of `buffer` (width, height, stride, format).
    pub fn OH_NativeBuffer_GetConfig(buffer: *mut OH_NativeBuffer, config: *mut OH_NativeBuffer_Config);

    /// Map `buffer` into CPU-accessible virtual memory; sets `*vir_addr`.
    /// Returns 0 on success.
    pub fn OH_NativeBuffer_Map(buffer: *mut OH_NativeBuffer, vir_addr: *mut *mut std::os::raw::c_void) -> i32;

    /// Unmap a previously mapped buffer.
    /// Returns 0 on success.
    pub fn OH_NativeBuffer_Unmap(buffer: *mut OH_NativeBuffer) -> i32;
}
