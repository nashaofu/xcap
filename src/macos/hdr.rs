//! HDR display capture through ScreenCaptureKit.
//!
//! The synchronous CoreGraphics grabs used for the normal freeze cannot see
//! extended-range pixels. ScreenCaptureKit gained HDR screenshots in macOS 15
//! (`SCScreenshotConfiguration.dynamicRange`), only on Apple Silicon; below
//! that the system has no public API that returns values above SDR white, so
//! the caller keeps the regular 8-bit path.

use std::sync::mpsc::sync_channel;
use std::time::Duration;

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained};
use objc2_core_graphics::{CGColorSpace, CGDataProvider, CGDirectDisplayID, CGImage};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotConfiguration, SCScreenshotDynamicRange, SCScreenshotManager,
    SCScreenshotOutput, SCShareableContent,
};

use crate::{
    HdrImage,
    error::{XCapError, XCapResult},
    hdr_image::{f16_to_f32, f32_to_f16_bits},
};

/// How long to wait for the screenshot completion handler.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether this Mac can capture HDR at all.
///
/// The SDK restricts HDR capture to Apple Silicon, and the screenshot
/// configuration class only exists from macOS 15.
pub fn supported() -> bool {
    cfg!(target_arch = "aarch64") && objc2::available!(macos = 15.0)
}

fn error_message(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

fn shareable_content() -> XCapResult<Retained<SCShareableContent>> {
    let (tx, rx) = sync_channel::<Result<Retained<SCShareableContent>, String>>(1);
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = if !error.is_null() {
                Err(error_message(unsafe { &*error }))
            } else if content.is_null() {
                Err("ScreenCaptureKit returned no shareable content".into())
            } else {
                Ok(unsafe { Retained::retain(content) }.expect("shareable content is not null"))
            };
            let _ = tx.send(result);
        },
    );
    // Same narrow enumeration as the video recorder: only `displays()` is
    // used here, but the full window list still costs ~15 ms to marshal.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        )
    };

    match rx.recv_timeout(CAPTURE_TIMEOUT) {
        Ok(Ok(content)) => Ok(content),
        Ok(Err(message)) => Err(XCapError::new(message)),
        Err(_) => Err(XCapError::new(
            "ScreenCaptureKit did not return shareable content in time",
        )),
    }
}

pub fn capture(display_id: CGDirectDisplayID) -> XCapResult<HdrImage> {
    if !supported() {
        log::info!("xcap macOS HDR: unsupported (need aarch64 + macOS 15+)");
        return Err(XCapError::NotSupported);
    }
    let content = shareable_content()?;
    let display = unsafe {
        content
            .displays()
            .iter()
            .find(|display| display.displayID() == display_id)
            .ok_or_else(|| XCapError::new("no shareable display with the requested id"))?
    };
    log::info!(
        "xcap macOS HDR: display {display_id}, config dynamicRange=SDRAndHDR showsCursor=false"
    );

    let (tx, rx) = sync_channel::<Result<HdrImage, String>>(1);
    let handler = RcBlock::new(
        move |output: *mut SCScreenshotOutput, error: *mut NSError| {
            let result = if !error.is_null() {
                let msg = error_message(unsafe { &*error });
                log::warn!("xcap macOS HDR: completion error: {msg}");
                Err(msg)
            } else if output.is_null() {
                log::warn!("xcap macOS HDR: completion returned no output");
                Err("ScreenCaptureKit returned no screenshot".into())
            } else {
                let output = unsafe { &*output };
                let hdr = unsafe { output.hdrImage() };
                log::info!(
                    "xcap macOS HDR: output hdrImage={} sdrImage={}",
                    if hdr.is_some() { "present" } else { "nil" },
                    if unsafe { output.sdrImage() }.is_some() { "present" } else { "nil" }
                );
                match hdr {
                    Some(image) => hdr_from_cg_image(&image),
                    None => Err("the screenshot came back without an HDR image".into()),
                }
            };
            let _ = tx.send(result);
        },
    );

    unsafe {
        let filter = SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::new(),
        );
        let configuration = SCScreenshotConfiguration::new();
        // `HDR` alone leaves `hdrImage` nil on some systems; Apple's own
        // examples ask for both and read `hdrImage` from that output.
        configuration.setDynamicRange(SCScreenshotDynamicRange::SDRAndHDR);
        configuration.setShowsCursor(false);
        SCScreenshotManager::captureScreenshotWithFilter_configuration_completionHandler(
            &filter,
            &configuration,
            Some(&handler),
        );
    }

    match rx.recv_timeout(CAPTURE_TIMEOUT) {
        Ok(Ok(image)) => Ok(image),
        Ok(Err(message)) => Err(XCapError::new(message)),
        Err(_) => Err(XCapError::new(
            "ScreenCaptureKit did not return an HDR screenshot in time",
        )),
    }
}

/// Convert the RGhA half-float screenshot into linear scRGB.
fn hdr_from_cg_image(image: &CGImage) -> Result<HdrImage, String> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let stride = CGImage::bytes_per_row(Some(image));
    let bits_per_component = CGImage::bits_per_component(Some(image));
    let bits_per_pixel = CGImage::bits_per_pixel(Some(image));
    if width == 0 || height == 0 {
        return Err("ScreenCaptureKit returned an empty HDR image".into());
    }
    if bits_per_component != 16 || bits_per_pixel != 64 {
        return Err(format!(
            "unexpected HDR screenshot layout: {bits_per_component} bits per component, {bits_per_pixel} bits per pixel"
        ));
    }
    if stride < width * 8 {
        return Err(format!(
            "HDR screenshot stride {stride} is too small for {width} pixels"
        ));
    }

    let provider = CGImage::data_provider(Some(image))
        .ok_or_else(|| "HDR screenshot has no data provider".to_string())?;
    let data = CGDataProvider::data(Some(&provider))
        .ok_or_else(|| "HDR screenshot has no data".to_string())?;
    let len = data.length() as usize;
    if len < (height - 1) * stride + width * 8 {
        return Err(format!(
            "HDR screenshot data is {len} bytes, too short for {width}x{height}"
        ));
    }
    // SAFETY: the provider owns `len` bytes for as long as `data` is alive.
    let raw = unsafe { std::slice::from_raw_parts(data.byte_ptr(), len) };

    // `SCCaptureDynamicRangeHDR` documents the image as extended sRGB; the
    // extended *linear* variant would already hold scRGB values.
    let srgb_lut =
        (!is_linear(CGImage::color_space(Some(image)).as_deref())).then(srgb_to_linear_lut);

    let mut pixels = Vec::with_capacity(width * height * 6);
    // `chunks` rather than `chunks_exact`: a provider is allowed to trim the
    // padding of its last row, which would otherwise drop that row.
    for row in raw.chunks(stride).take(height) {
        if row.len() < width * 8 {
            break;
        }
        for pixel in row[..width * 8].as_chunks::<8>().0 {
            for channel in pixel.as_chunks::<2>().0.iter().take(3) {
                let bits = u16::from_le_bytes(*channel);
                let out = match &srgb_lut {
                    Some(lut) => lut[bits as usize],
                    None => bits,
                };
                pixels.extend_from_slice(&out.to_le_bytes());
            }
        }
    }
    if pixels.len() != width * height * 6 {
        return Err(format!(
            "HDR screenshot is missing pixels: {} by {} came back short",
            width, height
        ));
    }
    Ok(HdrImage::new(width as u32, height as u32, pixels, 203.0))
}

/// Extended sRGB → linear scRGB for every half-float bit pattern.
///
/// The transfer function is two `powf` calls per sample, and a 5K frame has
/// ~45 M samples. Samples are half-float, so one 128 KiB table over their
/// 16-bit patterns turns the conversion into a lookup. The video recorder
/// builds it once per recording; the still capture builds one per screenshot.
pub(super) fn srgb_to_linear_lut() -> Box<[u16; 65_536]> {
    let mut lut = Box::new([0u16; 65_536]);
    for (bits, slot) in lut.iter_mut().enumerate() {
        *slot = f32_to_f16_bits(srgb_to_linear(f16_to_f32(bits as u16)));
    }
    lut
}

/// Whether a Core Graphics color space holds linear samples. ScreenCaptureKit
/// documents HDR output as extended sRGB, but some paths hand back the linear
/// extension instead, and the two need opposite conversions.
pub(super) fn is_linear(color_space: Option<&CGColorSpace>) -> bool {
    color_space
        .and_then(|space| CGColorSpace::name(Some(space)))
        .is_some_and(|name| name.to_string().contains("Linear"))
}

/// Extended-range sRGB transfer function, including values above 1.0.
fn srgb_to_linear(value: f32) -> f32 {
    let magnitude = value.abs();
    let linear = if magnitude <= 0.040_45 {
        magnitude / 12.92
    } else {
        ((magnitude + 0.055) / 1.055).powf(2.4)
    };
    linear.copysign(value)
}

#[cfg(test)]
mod tests {
    use super::srgb_to_linear;

    #[test]
    fn extended_srgb_decodes_past_sdr_white() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // The extended curve keeps going: 2.0 -> ~4.95 linear.
        assert!((srgb_to_linear(2.0) - 4.95).abs() < 0.02);
        // Negative components stay symmetric for wide-gamut colors.
        assert!((srgb_to_linear(-1.0) + 1.0).abs() < 1e-6);
    }
}
