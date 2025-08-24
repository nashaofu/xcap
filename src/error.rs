use std::sync::PoisonError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum XCapError {
    #[error("Not supported")]
    NotSupported,
    #[error("{0}")]
    Error(String),
    #[error("StdSyncPoisonError {0}")]
    StdSyncPoisonError(String),
    #[error("Invalid capture region: {0}")]
    InvalidCaptureRegion(String),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    XcbError(#[from] xcb::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    XcbConnError(#[from] xcb::ConnError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    ImageImageError(#[from] image::ImageError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    StdStringFromUtf8Error(#[from] std::string::FromUtf8Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    ZbusError(#[from] zbus::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    StdIOError(#[from] std::io::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    StdTimeSystemTimeError(#[from] std::time::SystemTimeError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LibwayshotError(#[from] libwayshot_xcap::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    ZbusZvariantError(#[from] zbus::zvariant::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    PipewireError(#[from] pipewire::Error),

    #[cfg(target_os = "macos")]
    #[error("Objc2CoreGraphicsCGError {:?}", 0)]
    Objc2CoreGraphicsCGError(objc2_core_graphics::CGError),

    #[cfg(target_os = "windows")]
    #[error(transparent)]
    WindowsCoreError(#[from] windows::core::Error),
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    Utf16Error(#[from] widestring::error::Utf16Error),
}

impl XCapError {
    pub fn new<S: ToString>(err: S) -> Self {
        XCapError::Error(err.to_string())
    }
}

pub type XCapResult<T> = Result<T, XCapError>;

impl<T> From<PoisonError<T>> for XCapError {
    fn from(value: PoisonError<T>) -> Self {
        XCapError::StdSyncPoisonError(value.to_string())
    }
}
