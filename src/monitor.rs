use std::sync::mpsc::Receiver;

use image::RgbaImage;

use crate::{
    HdrImage, VideoRecorder, error::XCapResult, platform::impl_monitor::ImplMonitor,
    video_recorder::Frame,
};

#[derive(Debug, Clone)]
pub struct Monitor {
    pub(crate) impl_monitor: ImplMonitor,
}

impl Monitor {
    pub(crate) fn new(impl_monitor: ImplMonitor) -> Monitor {
        Monitor { impl_monitor }
    }
}

impl Monitor {
    pub fn all() -> XCapResult<Vec<Monitor>> {
        let monitors = ImplMonitor::all()?
            .iter()
            .map(|impl_monitor| Monitor::new(impl_monitor.clone()))
            .collect();

        Ok(monitors)
    }

    pub fn from_point(x: i32, y: i32) -> XCapResult<Monitor> {
        let impl_monitor = ImplMonitor::from_point(x, y)?;

        Ok(Monitor::new(impl_monitor))
    }
}

impl Monitor {
    /// Unique identifier associated with the screen.
    pub fn id(&self) -> XCapResult<u32> {
        self.impl_monitor.id()
    }
    /// The display name
    pub fn name(&self) -> XCapResult<String> {
        self.impl_monitor.name()
    }
    /// The display friendly name
    pub fn friendly_name(&self) -> XCapResult<String> {
        self.impl_monitor.friendly_name()
    }
    /// The screen x coordinate.
    pub fn x(&self) -> XCapResult<i32> {
        self.impl_monitor.x()
    }
    /// The screen x coordinate.
    pub fn y(&self) -> XCapResult<i32> {
        self.impl_monitor.y()
    }
    /// The screen pixel width.
    pub fn width(&self) -> XCapResult<u32> {
        self.impl_monitor.width()
    }
    /// The screen pixel height.
    pub fn height(&self) -> XCapResult<u32> {
        self.impl_monitor.height()
    }
    /// Can be 0, 90, 180, 270, represents screen rotation in clock-wise degrees.
    pub fn rotation(&self) -> XCapResult<f32> {
        self.impl_monitor.rotation()
    }
    /// Output device's pixel scale factor.
    pub fn scale_factor(&self) -> XCapResult<f32> {
        self.impl_monitor.scale_factor()
    }
    /// The screen refresh rate.
    pub fn frequency(&self) -> XCapResult<f32> {
        self.impl_monitor.frequency()
    }
    /// Whether the screen is the main screen
    pub fn is_primary(&self) -> XCapResult<bool> {
        self.impl_monitor.is_primary()
    }

    /// Whether the screen is builtin
    pub fn is_builtin(&self) -> XCapResult<bool> {
        self.impl_monitor.is_builtin()
    }
}

impl Monitor {
    /// Capture the monitor as a standard SDR `RgbaImage`.
    ///
    /// On HDR monitors (non-WGC path), the HDR content is tone-mapped to SDR using
    /// a Reinhard operator so the result is always an 8-bit RGBA image.
    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        self.impl_monitor.capture_image()
    }

    pub fn capture_region(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<RgbaImage> {
        self.impl_monitor.capture_region(x, y, width, height)
    }

    /// Capture the full monitor and return raw HDR pixel data as an [`HdrImage`].
    ///
    /// On an HDR display (non-WGC path), pixels are in scRGB linear color space with
    /// f16 precision — values above 1.0 represent HDR highlights beyond 80 nits.
    ///
    /// On an SDR display, values are within [0, 1] expressed as f16.
    ///
    /// Returns [`crate::XCapError::NotSupported`] when compiled with the `wgc` feature,
    /// since WGC captures only in BGRA8.
    pub fn capture_image_hdr(&self) -> XCapResult<HdrImage> {
        self.impl_monitor.capture_image_hdr()
    }

    /// Capture a region of the monitor as raw HDR pixel data.
    ///
    /// Coordinates are in physical pixels relative to the top-left of the monitor.
    /// On Windows (non-WGC), the region is extracted on the GPU via `CopySubresourceRegion`
    /// so only the requested pixels are transferred to the CPU.
    ///
    /// Returns [`crate::XCapError::NotSupported`] on macOS, Linux, and WGC builds.
    pub fn capture_region_hdr(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<HdrImage> {
        self.impl_monitor.capture_region_hdr(x, y, width, height)
    }

    /// Peak display luminance in nits.
    ///
    /// - **Windows**: read from DXGI `IDXGIOutput6::GetDesc1` (`MaxLuminance`).
    /// - **macOS**: `NSScreen.maximumPotentialEDRValue × 203` (PQ reference white).
    /// - **Linux**: parsed from the EDID HDR Static Metadata Data Block (CTA-861.3).
    ///
    /// Returns `0.0` when the display is SDR or the value cannot be determined.
    pub fn peak_nits(&self) -> f64 {
        self.impl_monitor.peak_nits()
    }

    /// Returns `true` if this monitor is currently detected as HDR by the Windows
    /// capture path.
    ///

    /// On Windows (non-`wgc` builds), detection is based on both the DXGI output
    /// color space reported by `IDXGIOutput6::GetDesc1()` and whether Desktop
    /// Duplication `DuplicateOutput1` accepts HDR pixel formats, with special
    /// handling for YCbCr HDR outputs.
    ///
    /// As a result, `false` does not necessarily mean the panel is incapable of
    /// HDR; it can also mean the current mode / driver path does not expose HDR in
    /// a way that this capture backend can use. Always returns `false` on
    /// non-Windows platforms or when built with `wgc`.
    pub fn is_hdr(&self) -> bool {
        self.impl_monitor.is_hdr()
    }

    /// Query DXGI Desktop Duplication format support for this monitor.
    ///
    /// Returns panel bit depth, luminance range, color space, and which
    /// `DuplicateOutput1` formats the driver accepts. Windows-only.
    #[cfg(target_os = "windows")]
    pub fn dxgi_format_support(&self) -> Option<crate::DxgiFormatSupport> {
        self.impl_monitor.dxgi_format_support()
    }

    pub fn video_recorder(&self) -> XCapResult<(VideoRecorder, Receiver<Frame>)> {
        let (impl_video_recorder, sx) = self.impl_monitor.video_recorder()?;

        Ok((VideoRecorder::new(impl_video_recorder), sx))
    }
}

#[cfg(test)]
mod tests {
    use crate::XCapError;

    use super::*;

    #[test]
    fn test_capture_region_out_of_bounds() {
        let monitors = Monitor::all().unwrap();
        let monitor = &monitors[0]; // Get first monitor

        // Try to capture a region that extends beyond monitor bounds
        let x = monitor.width().unwrap() / 2;
        let y = monitor.height().unwrap() / 2;
        let width = monitor.width().unwrap();
        let height = monitor.height().unwrap();

        let result = monitor.capture_region(x, y, width, height);

        match result {
            Err(XCapError::InvalidCaptureRegion(_)) => (),
            _ => panic!("Expected InvalidCaptureRegion error"),
        }
    }
}
