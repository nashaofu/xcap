use thiserror::Error;

#[derive(Debug, Error)]
pub enum XCapError {
    #[error("{0}")]
    Error(String),

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
    StdStrUtf8Error(#[from] std::str::Utf8Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    DbusError(#[from] dbus::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    StdIOError(#[from] std::io::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    StdTimeSystemTimeError(#[from] std::time::SystemTimeError),

    #[cfg(target_os = "macos")]
    #[error("CoreGraphicsDisplayCGError {0}")]
    CoreGraphicsDisplayCGError(core_graphics::display::CGError),

    #[cfg(target_os = "windows")]
    #[error(transparent)]
    WindowsCoreError(#[from] windows::core::Error),
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    StdStringFromUtf16Error(#[from] std::string::FromUtf16Error),
}

impl XCapError {
    pub fn new<S: ToString>(err: S) -> Self {
        XCapError::Error(err.to_string())
    }
}

#[cfg(target_os = "macos")]
impl From<core_graphics::display::CGError> for XCapError {
    fn from(value: core_graphics::display::CGError) -> Self {
        XCapError::CoreGraphicsDisplayCGError(value)
    }
}

pub type XCapResult<T> = Result<T, XCapError>;
