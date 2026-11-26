use std::sync::OnceLock;

use image::RgbaImage;

use crate::{XCapError, XCapResult};

/// A high-dynamic-range screen capture in scRGB.
///
/// Pixels are three IEEE 754 half-precision values in RGB order, little-endian,
/// six bytes per pixel. The color space is linear extended sRGB: `1.0` is the
/// platform's reference white (80 nits on Windows, SDR white on macOS), values
/// above it are HDR highlights and negative values are wide-gamut colors
/// outside the sRGB triangle. Windows composites SDR content above that
/// reference, so [`with_sdr_white`](Self::with_sdr_white) records where SDR
/// white actually sits in the frame.
#[derive(Debug, Clone)]
pub struct HdrImage {
    width: u32,
    height: u32,
    raw: Vec<u8>,
    white_nits: f32,
    sdr_white_nits: f32,
}

impl HdrImage {
    /// Build a frame from raw half-float samples (RGB, little-endian, six
    /// bytes per pixel) and the luminance of `1.0` in nits.
    ///
    /// The white level must be finite and greater than zero. Use
    /// [`try_new`](Self::try_new) when the value comes from an external source.
    pub fn new(width: u32, height: u32, raw: Vec<u8>, white_nits: f32) -> Self {
        Self::try_new(width, height, raw, white_nits).expect("valid HDR frame")
    }

    /// Fallible constructor for raw half-float samples and their reference
    /// white. Invalid luminance or sample lengths are rejected instead of
    /// poisoning later tone-mapping and HDR export maths.
    pub fn try_new(width: u32, height: u32, raw: Vec<u8>, white_nits: f32) -> XCapResult<Self> {
        let white_nits = checked_white(white_nits)?;
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(6));
        if expected != Some(raw.len()) {
            return Err(XCapError::new(format!(
                "HDR samples are 6 bytes per pixel: expected {expected:?}, got {}",
                raw.len()
            )));
        }
        Ok(Self {
            width,
            height,
            raw,
            white_nits,
            sdr_white_nits: white_nits,
        })
    }

    /// Declare the luminance SDR content occupies in this frame.
    ///
    /// `1.0` is the platform's reference white, which is the SDR white point
    /// everywhere except Windows HDR: the desktop scales SDR content to the
    /// user's SDR brightness before compositing, so ordinary UI sits well above
    /// the 80-nit scRGB unit. Everything that converts to SDR or decides
    /// whether a frame has highlights needs this level.
    pub fn with_sdr_white(self, nits: f32) -> Self {
        self.try_with_sdr_white(nits).expect("valid SDR white")
    }

    /// Fallible SDR-white setter. Values below the reference white are raised
    /// to it; non-finite and non-positive values are rejected.
    pub fn try_with_sdr_white(mut self, nits: f32) -> XCapResult<Self> {
        self.sdr_white_nits = checked_white(nits)?.max(self.white_nits);
        Ok(self)
    }

    /// Luminance of SDR white in nits; `reference_white_nits` when unset.
    pub fn sdr_white_nits(&self) -> f32 {
        self.sdr_white_nits
    }

    /// Luminance of `1.0` in nits.
    ///
    /// Windows scRGB defines the unit as 80 nits; macOS extended sRGB is
    /// relative to SDR white, which HDR10 treats as 203 nits. Exporters scaling
    /// this data to an absolute transfer function need the difference.
    pub fn reference_white_nits(&self) -> f32 {
        self.white_nits
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw half-float RGB samples, `width * height * 6` bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.raw
    }

    /// Read a pixel as linear scRGB `[r, g, b]`.
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 3] {
        let base = ((y as usize * self.width as usize) + x as usize) * 6;
        let raw = &self.raw;
        [
            f16_to_f32(u16::from_le_bytes([raw[base], raw[base + 1]])),
            f16_to_f32(u16::from_le_bytes([raw[base + 2], raw[base + 3]])),
            f16_to_f32(u16::from_le_bytes([raw[base + 4], raw[base + 5]])),
        ]
    }

    /// Whether any sample rises above SDR white.
    ///
    /// Callers use this to keep ordinary screenshots on the existing 8-bit
    /// path: an HDR display showing only SDR content gains nothing from a
    /// 16-bit file, and the size difference is large.
    pub fn has_highlights(&self) -> bool {
        // Positive half-floats order like their bit patterns, so the white
        // point is a comparison rather than a decode per sample.
        let limit = f32_to_f16_bits(self.sdr_white_nits / self.white_nits);
        self.raw.as_chunks::<2>().0.iter().any(|bits| {
            let value = u16::from_le_bytes(*bits);
            // Negative components are wide-gamut colors, not highlights.
            value & 0x8000 == 0 && value > limit
        })
    }

    /// Tone the frame down to an 8-bit RGBA image for the overlay, clipboard,
    /// pins, history and OCR.
    ///
    /// SDR content is scaled so the SDR white point lands on `1.0`, matching
    /// what an SDR capture would show (on Windows HDR that means undoing the
    /// desktop's SDR brightness boost); only highlights are clipped. That is
    /// the trade-off the system grab APIs make, and it keeps the editor preview
    /// consistent across displays.
    pub fn to_sdr(&self) -> RgbaImage {
        let width = self.width as usize;
        let height = self.height as usize;
        let mut out = vec![0u8; width * height * 4];
        if width == 0 || height == 0 {
            return RgbaImage::from_raw(self.width, self.height, out)
                .expect("empty image has no pixels");
        }

        let scale = self.white_nits / self.sdr_white_nits;

        let threads = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4)
            .clamp(2, 8)
            .min(height);
        let band_rows = height.div_ceil(threads.max(1));
        let raw = &self.raw;
        std::thread::scope(|scope| {
            for (band, chunk) in out.chunks_mut(band_rows * width * 4).enumerate() {
                let first_row = band * band_rows;
                scope.spawn(move || {
                    for (row, out_row) in chunk.chunks_mut(width * 4).enumerate() {
                        let at = (first_row + row) * width * 6;
                        let raw = &raw[at..at + width * 6];
                        for (px, out) in raw
                            .as_chunks::<6>()
                            .0
                            .iter()
                            .zip(out_row.as_chunks_mut::<4>().0)
                        {
                            out[0] = encode_sample(px[0], px[1], scale);
                            out[1] = encode_sample(px[2], px[3], scale);
                            out[2] = encode_sample(px[4], px[5], scale);
                            out[3] = 255;
                        }
                    }
                });
            }
        });
        RgbaImage::from_raw(self.width, self.height, out)
            .expect("RGBA buffer is the same size as the HDR frame")
    }

    /// Crop a rectangle given in pixels, clamped to the image bounds.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Option<HdrImage> {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width - x);
        let height = height.min(self.height - y);
        if width == 0 || height == 0 {
            return None;
        }
        let row_bytes = width as usize * 6;
        let src_stride = self.width as usize * 6;
        let mut raw = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let at = (y as usize + row) * src_stride + x as usize * 6;
            raw.extend_from_slice(&self.raw[at..at + row_bytes]);
        }
        Some(HdrImage::new(width, height, raw, self.white_nits).with_sdr_white(self.sdr_white_nits))
    }
}

fn checked_white(nits: f32) -> XCapResult<f32> {
    if nits.is_finite() && nits > 0.0 {
        Ok(nits)
    } else {
        Err(XCapError::new(format!(
            "HDR white luminance must be finite and positive, got {nits}"
        )))
    }
}

fn srgb_encode_table() -> &'static [u8; 4096] {
    static TABLE: OnceLock<[u8; 4096]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u8; 4096];
        for (index, slot) in table.iter_mut().enumerate() {
            *slot = linear_to_srgb_u8(index as f32 / 4095.0);
        }
        table
    })
}

fn encode_sample(hi: u8, lo: u8, scale: f32) -> u8 {
    let linear = (f16_to_f32(u16::from_le_bytes([hi, lo])) * scale).clamp(0.0, 1.0);
    srgb_encode_table()[(linear * 4095.0).round() as usize]
}

fn linear_to_srgb_u8(linear: f32) -> u8 {
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// IEEE 754 half-precision bits → f32.
pub(crate) fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = u32::from(bits & 0x3ff);
    let f32_bits = if exponent == 0 {
        if mantissa == 0 {
            sign
        } else {
            // Subnormal: normalize the mantissa into the f32 exponent range.
            let mut exponent = 127 - 15 + 1;
            let mut mantissa = mantissa;
            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }
            sign | (exponent << 23) | ((mantissa & 0x3ff) << 13)
        }
    } else if exponent == 31 {
        sign | 0x7f80_0000 | (mantissa << 13)
    } else {
        sign | ((u32::from(exponent) + 127 - 15) << 23) | (mantissa << 13)
    };
    f32::from_bits(f32_bits)
}

/// f32 → the bits of an IEEE 754 half-precision value (round toward zero).
pub(crate) fn f32_to_f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    if exponent >= 31 {
        sign | 0x7c00
    } else if exponent <= 0 {
        sign
    } else {
        sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32, values: &[[f32; 3]]) -> HdrImage {
        let mut raw = Vec::with_capacity(values.len() * 6);
        for [r, g, b] in values {
            raw.extend_from_slice(&f32_to_f16_bits(*r).to_le_bytes());
            raw.extend_from_slice(&f32_to_f16_bits(*g).to_le_bytes());
            raw.extend_from_slice(&f32_to_f16_bits(*b).to_le_bytes());
        }
        HdrImage::new(width, height, raw, 80.0)
    }

    #[test]
    fn white_levels_reject_non_finite_and_non_positive_values() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0] {
            assert!(HdrImage::try_new(1, 1, vec![0; 6], value).is_err());
            assert!(sample(1, 1, &[[0.0; 3]]).try_with_sdr_white(value).is_err());
        }
        assert_eq!(
            HdrImage::try_new(1, 1, vec![0; 6], 80.0)
                .unwrap()
                .try_with_sdr_white(40.0)
                .unwrap()
                .sdr_white_nits(),
            80.0
        );
        assert_eq!(
            HdrImage::try_new(1, 1, vec![0; 6], 80.0)
                .unwrap()
                .try_with_sdr_white(200.0)
                .unwrap()
                .sdr_white_nits(),
            200.0
        );
    }

    #[test]
    #[should_panic(expected = "valid HDR frame")]
    fn infallible_constructor_rejects_invalid_white() {
        HdrImage::new(1, 1, vec![0; 6], f32::NAN);
    }

    #[test]
    #[should_panic(expected = "valid SDR white")]
    fn infallible_setter_rejects_invalid_white() {
        sample(1, 1, &[[0.0; 3]]).with_sdr_white(0.0);
    }

    #[test]
    fn fallible_constructor_rejects_invalid_sample_length() {
        assert!(HdrImage::try_new(1, 1, vec![0; 5], 80.0).is_err());
        assert!(HdrImage::try_new(u32::MAX, u32::MAX, Vec::new(), 80.0).is_err());
    }

    #[test]
    fn half_float_round_trip() {
        for value in [0.0_f32, 0.5, 1.0, 1.5, 4.0, 16.0, 80.0] {
            assert!((f16_to_f32(f32_to_f16_bits(value)) - value).abs() < 0.01);
        }
        assert!(f16_to_f32(0x3c00) == 1.0);
        assert!(f16_to_f32(0xbc00) == -1.0);
    }

    #[test]
    fn highlights_only_above_sdr_white() {
        let sdr = sample(1, 1, &[[1.0, 0.2, 0.0]]);
        assert!(!sdr.has_highlights());
        let hdr = sample(1, 1, &[[1.0, 1.25, 0.0]]);
        assert!(hdr.has_highlights());
        let negative = sample(1, 1, &[[-0.5, 0.0, 0.0]]);
        assert!(!negative.has_highlights());
        let wide_gamut = sample(1, 1, &[[-2.0, 0.0, 0.0]]);
        assert!(!wide_gamut.has_highlights());
    }

    #[test]
    fn sdr_white_level_shifts_the_highlight_threshold() {
        // Windows composites SDR content at the user's SDR brightness (about
        // 200 nits by default), so 2.5 in scRGB units is ordinary UI there.
        let sdr = sample(1, 1, &[[2.0, 0.0, 0.0]]).with_sdr_white(200.0);
        assert!(!sdr.has_highlights());
        let hdr = sample(1, 1, &[[3.0, 0.0, 0.0]]).with_sdr_white(200.0);
        assert!(hdr.has_highlights());
    }

    #[test]
    fn sdr_conversion_keeps_sdr_white() {
        let image = sample(1, 1, &[[1.0, 1.0, 1.0]]);
        let sdr = image.to_sdr();
        assert_eq!(sdr.get_pixel(0, 0).0, [255, 255, 255, 255]);
        let image = sample(1, 1, &[[0.0, 0.0, 0.0]]);
        assert_eq!(image.to_sdr().get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn sdr_conversion_undoes_the_desktop_brightness_boost() {
        // scRGB white at 200 nits is the SDR white the user actually sees.
        let image = sample(1, 1, &[[2.5, 2.5, 2.5]]).with_sdr_white(200.0);
        assert_eq!(image.to_sdr().get_pixel(0, 0).0, [255, 255, 255, 255]);
        // 0.25 → 0.1 linear after the 80/200 scale → ~89 in sRGB.
        let dim = sample(1, 1, &[[0.25, 0.0, 0.0]]).with_sdr_white(200.0);
        let red = dim.to_sdr().get_pixel(0, 0).0[0];
        assert!((88..=90).contains(&red), "unexpected red {red}");
    }

    #[test]
    fn crop_keeps_rows_tight() {
        let image = sample(
            3,
            2,
            &[
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.5, 0.5, 0.5],
                [0.25, 0.0, 0.0],
                [0.75, 0.0, 0.0],
            ],
        );
        let cropped = image.crop(1, 1, 2, 1).expect("in bounds");
        assert_eq!((cropped.width(), cropped.height()), (2, 1));
        assert_eq!(cropped.sdr_white_nits(), image.sdr_white_nits());
        let [r, _, _] = cropped.pixel_f32(0, 0);
        assert!((r - 0.25).abs() < 0.01);
        let [r, _, _] = cropped.pixel_f32(1, 0);
        assert!((r - 0.75).abs() < 0.01);
        assert!(image.crop(3, 0, 1, 1).is_none());
    }
}
