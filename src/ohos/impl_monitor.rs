//! OHOS monitor/display implementation using `OH_NativeDisplayManager` APIs.

use std::ffi::CStr;
use std::sync::mpsc::Receiver;

use image::RgbaImage;

use crate::{
    error::{XCapError, XCapResult},
    video_recorder::Frame,
};

use super::{capture::capture_screen, ffi, impl_video_recorder::ImplVideoRecorder};

// ── Struct ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ImplMonitor {
    pub display_id: u32,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub refresh_rate: u32,
    pub density_pixels: f32,
    pub rotation: ffi::NativeDisplayManager_Rotation,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
}

impl ImplMonitor {
    fn from_raw(
        info: &ffi::NativeDisplayManager_DisplayInfo,
        is_primary: bool,
        x: i32,
        y: i32,
    ) -> Self {
        // SAFETY: `name` is a NUL-terminated C string written by the NDK.
        let name = unsafe {
            CStr::from_ptr(info.name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        ImplMonitor {
            display_id: info.id,
            name,
            width: info.width,
            height: info.height,
            refresh_rate: info.refreshRate,
            density_pixels: info.densityPixels,
            rotation: info.rotation,
            is_primary,
            x,
            y,
        }
    }

    // ── Constructors ──────────────────────────────────────────────────────────

    /// Return a list of all active (alive) displays on the device.
    pub fn all() -> XCapResult<Vec<ImplMonitor>> {
        // Determine the primary display ID so we can set `is_primary` correctly.
        let primary_id = unsafe { query_primary_id() };

        let mut all_displays: *mut ffi::NativeDisplayManager_DisplaysInfo = core::ptr::null_mut();
        let err = unsafe {
            ffi::OH_NativeDisplayManager_CreateAllDisplays(&mut all_displays)
        };

        if err != ffi::NativeDisplayManager_ErrorCode::Ok || all_displays.is_null() {
            return Err(XCapError::new(format!(
                "OH_NativeDisplayManager_CreateAllDisplays failed: {:?}",
                err
            )));
        }

        let mut monitors = Vec::new();

        // SAFETY: `all_displays` is a valid pointer returned by the NDK.
        unsafe {
            let len = (*all_displays).displaysLength;
            let infos = (*all_displays).displaysInfo;

            for i in 0..len {
                let info = &*infos.add(i as usize);

                // Skip disconnected displays.
                if !info.isAlive {
                    continue;
                }

                let mut x: i32 = 0;
                let mut y: i32 = 0;
                ffi::OH_NativeDisplayManager_GetDisplayPosition(
                    info.id as u64,
                    &mut x,
                    &mut y,
                );

                monitors.push(ImplMonitor::from_raw(
                    info,
                    info.id == primary_id,
                    x,
                    y,
                ));
            }

            ffi::OH_NativeDisplayManager_DestroyAllDisplays(all_displays);
        }

        Ok(monitors)
    }

    /// Return the monitor that contains the point `(x, y)` in global coordinates.
    pub fn from_point(x: i32, y: i32) -> XCapResult<ImplMonitor> {
        Self::all()?
            .into_iter()
            .find(|m| x >= m.x && x < m.x + m.width && y >= m.y && y < m.y + m.height)
            .ok_or_else(|| XCapError::new(format!("No monitor found at ({}, {})", x, y)))
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn id(&self) -> XCapResult<u32> {
        Ok(self.display_id)
    }

    pub fn name(&self) -> XCapResult<String> {
        Ok(self.name.clone())
    }

    pub fn friendly_name(&self) -> XCapResult<String> {
        Ok(self.name.clone())
    }

    pub fn x(&self) -> XCapResult<i32> {
        Ok(self.x)
    }

    pub fn y(&self) -> XCapResult<i32> {
        Ok(self.y)
    }

    pub fn width(&self) -> XCapResult<u32> {
        Ok(self.width as u32)
    }

    pub fn height(&self) -> XCapResult<u32> {
        Ok(self.height as u32)
    }

    /// Clockwise rotation in degrees (0, 90, 180, 270).
    pub fn rotation(&self) -> XCapResult<f32> {
        let degrees = match self.rotation {
            ffi::NativeDisplayManager_Rotation::Rotation0 => 0.0,
            ffi::NativeDisplayManager_Rotation::Rotation90 => 90.0,
            ffi::NativeDisplayManager_Rotation::Rotation180 => 180.0,
            ffi::NativeDisplayManager_Rotation::Rotation270 => 270.0,
        };
        Ok(degrees)
    }

    /// Logical pixel density (≈ DPR), e.g. `3.0` for a high-DPI phone screen.
    pub fn scale_factor(&self) -> XCapResult<f32> {
        Ok(self.density_pixels)
    }

    pub fn frequency(&self) -> XCapResult<f32> {
        Ok(self.refresh_rate as f32)
    }

    pub fn is_primary(&self) -> XCapResult<bool> {
        Ok(self.is_primary)
    }

    /// On OHOS phones/tablets the primary display is always the built-in panel.
    pub fn is_builtin(&self) -> XCapResult<bool> {
        Ok(self.is_primary)
    }

    // ── Capture ───────────────────────────────────────────────────────────────

    pub fn capture_image(&self) -> XCapResult<RgbaImage> {
        capture_screen(self.display_id as u64, self.width as u32, self.height as u32)
    }

    /// Capture a sub-region of this monitor.
    ///
    /// `x`, `y` are relative to the monitor's top-left corner.
    pub fn capture_region(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<RgbaImage> {
        if x + width > self.width as u32 || y + height > self.height as u32 {
            return Err(XCapError::InvalidCaptureRegion(format!(
                "Region ({x}, {y}, {width}, {height}) exceeds monitor bounds ({}x{})",
                self.width, self.height
            )));
        }

        let full = self.capture_image()?;
        Ok(image::imageops::crop_imm(&full, x, y, width, height).to_image())
    }

    pub fn video_recorder(&self) -> XCapResult<(ImplVideoRecorder, Receiver<Frame>)> {
        ImplVideoRecorder::new(self.display_id as u64, self.width as u32, self.height as u32)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Query the ID of the primary display, or 0 if the call fails.
///
/// # Safety
/// Calls into the OHOS NDK; must only be called on a valid runtime.
unsafe fn query_primary_id() -> u32 {
    let mut primary_info: *mut ffi::NativeDisplayManager_DisplayInfo = core::ptr::null_mut();
    let err = ffi::OH_NativeDisplayManager_CreatePrimaryDisplay(&mut primary_info);

    if err == ffi::NativeDisplayManager_ErrorCode::Ok && !primary_info.is_null() {
        let id = (*primary_info).id;
        ffi::OH_NativeDisplayManager_DestroyDisplay(primary_info);
        id
    } else {
        0
    }
}
