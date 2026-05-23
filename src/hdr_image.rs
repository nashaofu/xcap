use image::{Rgb32FImage, RgbImage, RgbaImage};

/// An HDR screen capture in scRGB linear color space.
///
/// Pixels are stored as 6 bytes each: three IEEE 754 half-precision (f16) values
/// in RGB channel order, little-endian. Values above 1.0 represent luminance
/// beyond the SDR white point (~80 nits). Values can be negative in scRGB (wide gamut).
///
/// Use [`HdrImage::pixel_f32`] to read individual pixels, or the conversion helpers
/// [`HdrImage::to_rgb_image`] and [`HdrImage::to_rgb_image_tonemapped`] to produce
/// a displayable SDR image.
#[derive(Debug, Clone)]
pub struct HdrImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw pixel bytes: `width * height * 6` bytes (f16 RGB, row-major, little-endian).
    pub raw: Vec<u8>,
}

impl HdrImage {
    #[cfg(target_os = "windows")]
    pub(crate) fn new(width: u32, height: u32, raw: Vec<u8>) -> Self {
        Self { width, height, raw }
    }

    /// Build an `HdrImage` from an sRGB RGBA8 image.
    ///
    /// Each channel is converted from sRGB gamma-corrected u8 to linear f32, then
    /// stored as f16 LE. The resulting values are in [0, 1] (no HDR highlights).
    pub(crate) fn from_srgb_rgba8(img: &RgbaImage) -> Self {
        let width = img.width();
        let height = img.height();
        let mut raw = vec![0u8; (width * height * 6) as usize];
        let mut dst = 0usize;
        for pixel in img.pixels() {
            let r = f32_to_f16_le(srgb_u8_to_linear(pixel[0]));
            let g = f32_to_f16_le(srgb_u8_to_linear(pixel[1]));
            let b = f32_to_f16_le(srgb_u8_to_linear(pixel[2]));
            raw[dst..dst + 2].copy_from_slice(&r);
            raw[dst + 2..dst + 4].copy_from_slice(&g);
            raw[dst + 4..dst + 6].copy_from_slice(&b);
            dst += 6;
        }
        Self { width, height, raw }
    }

    /// Read pixel `(x, y)` as `[R, G, B]` in linear scRGB f32 values.
    ///
    /// Values > 1.0 are HDR highlights. Values may be negative (wide-gamut scRGB).
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 3] {
        let base = ((y * self.width + x) * 6) as usize;
        let r = &self.raw;
        [
            f16_bytes_to_f32([r[base], r[base + 1]]),
            f16_bytes_to_f32([r[base + 2], r[base + 3]]),
            f16_bytes_to_f32([r[base + 4], r[base + 5]]),
        ]
    }

    /// Convert to a standard `RgbImage` by **clipping** HDR values to [0, 1] then
    /// applying sRGB gamma. Highlights above the SDR range are clipped to white.
    ///
    /// Prefer [`HdrImage::to_rgb_image_tonemapped`] for better quality.
    pub fn to_rgb_image(&self) -> RgbImage {
        let mut pixels = vec![0u8; (self.width * self.height * 3) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 3) as usize;
                pixels[base] = linear_to_srgb_u8(r.clamp(0.0, 1.0));
                pixels[base + 1] = linear_to_srgb_u8(g.clamp(0.0, 1.0));
                pixels[base + 2] = linear_to_srgb_u8(b.clamp(0.0, 1.0));
            }
        }
        RgbImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
    }

    /// Convert to a standard `RgbImage` using per-channel **Reinhard tone mapping**.
    ///
    /// This compresses HDR highlights into the SDR range while preserving relative
    /// luminance, giving a more natural result than hard clipping.
    ///
    /// # Parameters
    /// - `peak_luminance_nits`: approximate peak luminance of the captured content in nits.
    ///   In scRGB, 1.0 = 80 nits. For a typical HDR display at 400 nits, pass `400.0`.
    pub fn to_rgb_image_tonemapped(&self, peak_luminance_nits: f32) -> RgbImage {
        let peak = (peak_luminance_nits / 80.0).max(1.0);
        let mut pixels = vec![0u8; (self.width * self.height * 3) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 3) as usize;
                pixels[base] = linear_to_srgb_u8(reinhard(r.max(0.0), peak));
                pixels[base + 1] = linear_to_srgb_u8(reinhard(g.max(0.0), peak));
                pixels[base + 2] = linear_to_srgb_u8(reinhard(b.max(0.0), peak));
            }
        }
        RgbImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
    }

    /// Convert to a linear f32 `Rgb32FImage`, preserving full HDR range.
    ///
    /// Values above 1.0 are HDR highlights in scRGB linear space. Use this to
    /// save as tiff or for downstream HDR processing.
    pub fn to_rgb32f_image(&self) -> Rgb32FImage {
        let mut pixels = vec![0.0f32; (self.width * self.height * 3) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 3) as usize;
                pixels[base] = r;
                pixels[base + 1] = g;
                pixels[base + 2] = b;
            }
        }
        Rgb32FImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
    }
}

// ── Tone mapping ──────────────────────────────────────────────────────────────

/// Extended Reinhard: `x * (1 + x/peak²) / (1 + x)`, maps [0, ∞) → [0, 1).
fn reinhard(x: f32, peak: f32) -> f32 {
    (x * (1.0 + x / (peak * peak))) / (1.0 + x)
}

// ── Gamma conversion ──────────────────────────────────────────────────────────

fn linear_to_srgb_u8(linear: f32) -> u8 {
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

// ── f32 → f16 ─────────────────────────────────────────────────────────────────

/// f32 → IEEE 754 half-precision little-endian bytes.
fn f32_to_f16_le(v: f32) -> [u8; 2] {
    let bits = v.to_bits();
    let s = bits >> 31;
    let e = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    let m = bits & 0x7F_FFFF;
    let half: u16 = if e >= 31 {
        (s << 15) as u16 | 0x7C00 // clamp to Inf
    } else if e <= 0 {
        (s << 15) as u16 // flush to zero
    } else {
        ((s << 15) | ((e as u32) << 10) | (m >> 13)) as u16
    };
    half.to_le_bytes()
}

/// sRGB gamma u8 → linear f32 in [0, 1].
fn srgb_u8_to_linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

// ── f16 → f32 ─────────────────────────────────────────────────────────────────

/// IEEE 754 half-precision bytes (little-endian) → f32.
fn f16_bytes_to_f32(bytes: [u8; 2]) -> f32 {
    let bits = u16::from_le_bytes(bytes) as u32;
    let s = bits >> 15;
    let e = (bits >> 10) & 0x1F;
    let m = bits & 0x3FF;
    let f32_bits = if e == 0 {
        if m == 0 {
            s << 31 // ±zero
        } else {
            // Subnormal: find implicit leading 1
            let mut exp = 127u32.wrapping_sub(14);
            let mut mantissa = m;
            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exp = exp.wrapping_sub(1);
            }
            mantissa &= !0x400;
            (s << 31) | (exp << 23) | (mantissa << 13)
        }
    } else if e == 31 {
        (s << 31) | 0x7F80_0000 | (m << 13) // Inf or NaN
    } else {
        (s << 31) | ((e + 127 - 15) << 23) | (m << 13)
    };
    f32::from_bits(f32_bits)
}
