use image::{Rgba32FImage, RgbaImage};

/// An HDR screen capture in scRGB linear color space.
///
/// Pixels are stored as 8 bytes each: four IEEE 754 half-precision (f16) values
/// in RGBA channel order, little-endian. Values above 1.0 represent luminance
/// beyond the SDR white point (~80 nits). Values can be negative in scRGB (wide gamut).
///
/// Use [`HdrImage::pixel_f32`] to read individual pixels, or the conversion helpers
/// [`HdrImage::to_rgba_image`] and [`HdrImage::to_rgba_image_tonemapped`] to produce
/// a displayable SDR image.
#[derive(Debug, Clone)]
pub struct HdrImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw pixel bytes: `width * height * 8` bytes (f16 RGBA, row-major, little-endian).
    pub raw: Vec<u8>,
}

impl HdrImage {
    pub(crate) fn new(width: u32, height: u32, raw: Vec<u8>) -> Self {
        Self { width, height, raw }
    }

    /// Read pixel `(x, y)` as `[R, G, B, A]` in linear scRGB f32 values.
    ///
    /// Values > 1.0 are HDR highlights. Values may be negative (wide-gamut scRGB).
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let base = ((y * self.width + x) * 8) as usize;
        let r = &self.raw;
        [
            f16_bytes_to_f32([r[base], r[base + 1]]),
            f16_bytes_to_f32([r[base + 2], r[base + 3]]),
            f16_bytes_to_f32([r[base + 4], r[base + 5]]),
            f16_bytes_to_f32([r[base + 6], r[base + 7]]),
        ]
    }

    /// Convert to a standard `RgbaImage` by **clipping** HDR values to [0, 1] then
    /// applying sRGB gamma. Highlights above the SDR range are clipped to white.
    ///
    /// Prefer [`HdrImage::to_rgba_image_tonemapped`] for better quality.
    pub fn to_rgba_image(&self) -> RgbaImage {
        let mut pixels = vec![0u8; (self.width * self.height * 4) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b, a] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 4) as usize;
                pixels[base] = linear_to_srgb_u8(r.clamp(0.0, 1.0));
                pixels[base + 1] = linear_to_srgb_u8(g.clamp(0.0, 1.0));
                pixels[base + 2] = linear_to_srgb_u8(b.clamp(0.0, 1.0));
                pixels[base + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
        RgbaImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
    }

    /// Convert to a standard `RgbaImage` using per-channel **Reinhard tone mapping**.
    ///
    /// This compresses HDR highlights into the SDR range while preserving relative
    /// luminance, giving a more natural result than hard clipping.
    ///
    /// # Parameters
    /// - `peak_luminance_nits`: approximate peak luminance of the captured content in nits.
    ///   In scRGB, 1.0 = 80 nits. For a typical HDR display at 400 nits, pass `400.0`.
    pub fn to_rgba_image_tonemapped(&self, peak_luminance_nits: f32) -> RgbaImage {
        let peak = (peak_luminance_nits / 80.0).max(1.0);
        let mut pixels = vec![0u8; (self.width * self.height * 4) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b, a] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 4) as usize;
                pixels[base] = linear_to_srgb_u8(reinhard(r.max(0.0), peak));
                pixels[base + 1] = linear_to_srgb_u8(reinhard(g.max(0.0), peak));
                pixels[base + 2] = linear_to_srgb_u8(reinhard(b.max(0.0), peak));
                pixels[base + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
        RgbaImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
    }

    /// Convert to a linear f32 `Rgba32FImage`, preserving full HDR range.
    ///
    /// Values above 1.0 are HDR highlights in scRGB linear space. Use this to
    /// save as tiff or for downstream HDR processing.
    pub fn to_rgba32f_image(&self) -> Rgba32FImage {
        let mut pixels = vec![0.0f32; (self.width * self.height * 4) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b, a] = self.pixel_f32(x, y);
                let base = ((y * self.width + x) * 4) as usize;
                pixels[base] = r;
                pixels[base + 1] = g;
                pixels[base + 2] = b;
                pixels[base + 3] = a;
            }
        }
        Rgba32FImage::from_raw(self.width, self.height, pixels).expect("dimensions match")
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
