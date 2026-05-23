use image::RgbaImage;
use log::log;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709,
                DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                DXGI_COLOR_SPACE_RGB_STUDIO_G2084_NONE_P2020,
                DXGI_COLOR_SPACE_YCBCR_FULL_GHLG_TOPLEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_TOPLEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020,
                DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT, DXGI_ERROR_UNSUPPORTED,
                DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, IDXGIOutput1,
                IDXGIOutput5, IDXGIOutput6, IDXGIOutputDuplication, IDXGIResource,
            },
            Gdi::HMONITOR,
        },
    },
    core::{HRESULT, Interface},
};
use crate::{
    HdrImage,
    error::{XCapError, XCapResult},
};

use super::utils::bgra_to_rgba;

/// Result of a DXGI Desktop Duplication capture.
pub(super) enum CaptureFrame {
    /// Standard Dynamic Range — BGRA8 converted to RGBA8.
    Sdr(RgbaImage),
    /// High Dynamic Range — scRGB linear, stored as f16 RGB (6 bytes/pixel; alpha is dropped).
    Hdr(HdrImage),
}

/// Which pixel format the DXGI duplication session was opened with.
#[derive(Clone, Copy, PartialEq)]
enum HdrMode {
    /// `R16G16B16A16_FLOAT` — scRGB linear, 8 bytes/pixel.
    Float,
    /// `R10G10B10A2_UNORM` — HDR10 PQ-encoded, 4 bytes/pixel.
    Pq,
    /// `B8G8R8A8_UNORM` — SDR, 4 bytes/pixel.
    Sdr,
}

// ── Internal session ──────────────────────────────────────────────────────────

struct DxgiSession {
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    hdr_mode: HdrMode,
}

impl DxgiSession {
    /// Find the DXGI output for `h_monitor`, create a D3D11 device on its adapter,
    /// detect HDR mode via `IDXGIOutput6`, and open a desktop duplication session.
    ///
    /// Enumerates every adapter so multi-GPU systems are handled correctly.
    fn new(h_monitor: HMONITOR) -> XCapResult<Self> {
        unsafe {
            let factory = CreateDXGIFactory1::<IDXGIFactory1>()?;

            let mut adapter_idx = 0u32;
            loop {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_idx) {
                    Ok(a) => a,
                    Err(e) if is_not_found(e.code()) => {
                        return Err(XCapError::new("DXGI: monitor not found on any adapter"));
                    }
                    Err(e) => return Err(e.into()),
                };
                adapter_idx += 1;

                let mut output_idx = 0u32;
                loop {
                    let output = match adapter.EnumOutputs(output_idx) {
                        Ok(o) => o,
                        Err(e) if is_not_found(e.code()) => break,
                        Err(e) => return Err(e.into()),
                    };
                    output_idx += 1;

                    let desc = output.GetDesc()?;
                    if desc.Monitor != h_monitor {
                        continue;
                    }

                    // ── Found matching output ─────────────────────────────
                    // Detect HDR: IDXGIOutput6::GetDesc1() exposes the color space.
                    // DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709 = scRGB linear, the
                    // color space Windows uses when the display is in HDR mode.
                    let is_hdr_display = output
                        .cast::<IDXGIOutput6>()
                        .ok()
                        .and_then(|o6| o6.GetDesc1().ok())
                        .map(|d| is_hdr_color_space(d.ColorSpace))
                        .unwrap_or(false);

                    // Create a D3D11 device bound to this specific adapter.
                    // D3D_DRIVER_TYPE_UNKNOWN is required when an explicit adapter is given.
                    let mut d3d_device_opt = None;
                    D3D11CreateDevice(
                        Some(&adapter.cast()?),
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut d3d_device_opt),
                        None,
                        None,
                    )?;
                    let d3d_device = d3d_device_opt
                        .ok_or_else(|| XCapError::new("D3D11CreateDevice returned None"))?;
                    let d3d_context = d3d_device.GetImmediateContext()?;
                    let dxgi_device = d3d_device.cast::<IDXGIDevice>()?;

                    // ── Open desktop duplication ─────────────────────────
                    // For HDR, try IDXGIOutput5::DuplicateOutput1 formats in order:
                    //   1. R16G16B16A16_FLOAT — scRGB linear (best quality)
                    //   2. R10G10B10A2_UNORM  — HDR10 PQ (fallback for 8-bit/FRC panels)
                    // Fall back to IDXGIOutput1::DuplicateOutput (always BGRA8) if
                    // IDXGIOutput5 is unavailable or both formats are rejected.
                    log::debug!("DxgiSession::new is_hdr_display={is_hdr_display}");
                    let (duplication, hdr_mode) = if is_hdr_display {
                        match output.cast::<IDXGIOutput5>() {
                            Ok(output5) => {
                                match output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R16G16B16A16_FLOAT]) {
                                    Ok(dup) => { log::debug!("DxgiSession: opened Float"); (dup, HdrMode::Float) }
                                    Err(e) => {
                                        log::debug!("DuplicateOutput1 rejected R16G16B16A16_FLOAT: {e} ({:#010x})", e.code().0);
                                        match output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R10G10B10A2_UNORM]) {
                                            Ok(dup) => { log::debug!("DxgiSession: opened Pq"); (dup, HdrMode::Pq) }
                                            Err(e2) => {
                                                log::debug!("DuplicateOutput1 rejected R10G10B10A2_UNORM: {e2} ({:#010x})", e2.code().0);
                                                let o1 = output.cast::<IDXGIOutput1>()?;
                                                match o1.DuplicateOutput(&dxgi_device) {
                                                    Ok(dup) => { log::debug!("DxgiSession: opened Sdr fallback"); (dup, HdrMode::Sdr) }
                                                    Err(e3) => { log::debug!("DuplicateOutput (Sdr fallback) failed: {e3} ({:#010x})", e3.code().0); return Err(e3.into()); }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::debug!("DxgiSession: IDXGIOutput5 cast failed: {e}");
                                let o1 = output.cast::<IDXGIOutput1>()?;
                                match o1.DuplicateOutput(&dxgi_device) {
                                    Ok(dup) => { log::debug!("DxgiSession: opened Sdr (no Output5)"); (dup, HdrMode::Sdr) }
                                    Err(e2) => { log::debug!("DuplicateOutput (no Output5 fallback) failed: {e2} ({:#010x})", e2.code().0); return Err(e2.into()); }
                                }
                            }
                        }
                    } else {
                        log::debug!("DxgiSession: SDR display, opening Sdr session");
                        let o1 = output.cast::<IDXGIOutput1>()?;
                        match o1.DuplicateOutput(&dxgi_device) {
                            Ok(dup) => { log::debug!("DxgiSession: opened Sdr"); (dup, HdrMode::Sdr) }
                            Err(e) => { log::debug!("DuplicateOutput (Sdr) failed: {e} ({:#010x})", e.code().0); return Err(e.into()); }
                        }
                    };

                    return Ok(DxgiSession {
                        d3d_device,
                        d3d_context,
                        duplication,
                        hdr_mode,
                    });
                }
            }
        }
    }

    /// Acquire the next desktop frame and copy the requested ROI to a CPU-accessible
    /// staging texture.  `x`, `y`, `width`, `height` are monitor-relative.
    fn capture_frame(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<CaptureFrame> {
        unsafe {
            let bytes_per_pixel = if self.hdr_mode == HdrMode::Float { 8usize } else { 4usize };
            let row_bytes = width as usize * bytes_per_pixel;
            let mut raw = vec![0u8; row_bytes * height as usize];

            // Staging texture is created on the first iteration and reused on retries
            // to avoid redundant GPU allocations.
            let mut staging_opt: Option<ID3D11Texture2D> = None;

            // On some drivers (notably AMD) the very first AcquireNextFrame after a
            // fresh DuplicateOutput session returns an all-zero frame buffer even
            // though the call reports success.  We detect this and re-acquire up to
            // MAX_ATTEMPTS times before giving up (which triggers a GDI fallback).
            const MAX_ATTEMPTS: usize = 5;

            for attempt in 0..MAX_ATTEMPTS {
                if attempt > 0 {
                    // Give the driver a frame period to populate the buffer.
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }

                // ── 1. Acquire next desktop frame ─────────────────────────
                let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut resource: Option<IDXGIResource> = None;
                let mut acquired = false;

                for _ in 0..10 {
                    match self
                        .duplication
                        .AcquireNextFrame(0, &mut frame_info, &mut resource)
                    {
                        Ok(()) => {
                            acquired = true;
                            break;
                        }
                        Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                if !acquired {
                    return Err(XCapError::new(
                        "DXGI AcquireNextFrame timed out: display may be asleep or unchanged",
                    ));
                }

                let resource = resource
                    .ok_or_else(|| XCapError::new("AcquireNextFrame returned no resource"))?;
                let desktop_texture: ID3D11Texture2D = resource.cast()?;

                // ── 2. Validate ROI against actual desktop texture size ────
                // CopySubresourceRegion fails silently for out-of-bounds regions,
                // which leaves the staging texture all zeros.  Detect this early.
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                desktop_texture.GetDesc(&mut src_desc);

                if x + width > src_desc.Width || y + height > src_desc.Height {
                    self.duplication.ReleaseFrame()?;
                    return Err(XCapError::new(format!(
                        "DXGI ROI ({}..{}, {}..{}) exceeds desktop texture {}×{}",
                        x,
                        x + width,
                        y,
                        y + height,
                        src_desc.Width,
                        src_desc.Height,
                    )));
                }

                // ── 3. Create (or reuse) CPU-readable staging texture ──────
                let staging = match staging_opt.take() {
                    Some(t) => t,
                    None => {
                        let mut staging_desc = D3D11_TEXTURE2D_DESC {
                            Width: width,
                            Height: height,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: src_desc.Format,
                            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_STAGING,
                            BindFlags: 0,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                            MiscFlags: 0,
                        };
                        staging_desc.MipLevels = 1; // ensure
                        let mut tex = None;
                        self.d3d_device
                            .CreateTexture2D(&staging_desc, None, Some(&mut tex))?;
                        tex.ok_or_else(|| XCapError::new("CreateTexture2D returned None"))?
                    }
                };

                // ── 4. GPU copy ROI → staging ─────────────────────────────
                let region = D3D11_BOX {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + height,
                    front: 0,
                    back: 1,
                };
                self.d3d_context.CopySubresourceRegion(
                    Some(&staging.cast()?),
                    0,
                    0,
                    0,
                    0,
                    Some(&desktop_texture.cast()?),
                    0,
                    Some(&region),
                );

                // ── 5. Release DXGI frame before mapping staging ──────────
                self.duplication.ReleaseFrame()?;

                // ── 6. Map staging and read pixel rows ────────────────────
                let staging_res: windows::Win32::Graphics::Direct3D11::ID3D11Resource =
                    staging.cast()?;
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.d3d_context.Map(
                    Some(&staging_res),
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut mapped),
                )?;

                let src_ptr = mapped.pData as *const u8;
                for row in 0..height as usize {
                    std::ptr::copy_nonoverlapping(
                        src_ptr.add(row * mapped.RowPitch as usize),
                        raw.as_mut_ptr().add(row * row_bytes),
                        row_bytes,
                    );
                }
                self.d3d_context.Unmap(Some(&staging_res), 0);

                // ── 7. Detect empty frame ─────────────────────────────────
                // A real black frame has alpha = 0xFF (BGRA8) or 0x3C00 (f16),
                // so all-zero bytes reliably identify an uninitialized frame buffer.
                if raw.iter().all(|&b| b == 0) {
                    log::debug!("DXGI: empty frame on attempt {attempt}/{MAX_ATTEMPTS}, retrying");
                    staging_opt = Some(staging_res.cast()?);
                    continue;
                }

                // ── 8. Produce output ─────────────────────────────────────
                return match self.hdr_mode {
                    HdrMode::Float => {
                        // R16G16B16A16_FLOAT: strip alpha, keep R, G, B f16s (6 bytes/pixel).
                        let rgb_raw: Vec<u8> = raw
                            .chunks_exact(8)
                            .flat_map(|p| [p[0], p[1], p[2], p[3], p[4], p[5]])
                            .collect();
                        Ok(CaptureFrame::Hdr(HdrImage::new(width, height, rgb_raw)))
                    }
                    HdrMode::Pq => {
                        // R10G10B10A2_UNORM: 4 bytes/pixel, R=bits[0..9], G=bits[10..19], B=bits[20..29].
                        // Apply PQ EOTF to convert HDR10 → linear light, then scale to scRGB
                        // (1.0 = 80 nits; PQ peak = 10 000 nits → multiply by 10000/80 = 125).
                        let rgb_raw: Vec<u8> = raw
                            .chunks_exact(4)
                            .flat_map(|p| {
                                let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                                let r = pq_eotf((v & 0x3FF) as f32 / 1023.0) * 125.0;
                                let g = pq_eotf(((v >> 10) & 0x3FF) as f32 / 1023.0) * 125.0;
                                let b = pq_eotf(((v >> 20) & 0x3FF) as f32 / 1023.0) * 125.0;
                                let rf = f32_to_f16_le(r);
                                let gf = f32_to_f16_le(g);
                                let bf = f32_to_f16_le(b);
                                [rf[0], rf[1], gf[0], gf[1], bf[0], bf[1]]
                            })
                            .collect();
                        Ok(CaptureFrame::Hdr(HdrImage::new(width, height, rgb_raw)))
                    }
                    HdrMode::Sdr => {
                        // B8G8R8A8_UNORM: swap B↔R.
                        Ok(CaptureFrame::Sdr(
                            RgbaImage::from_raw(width, height, bgra_to_rgba(raw))
                                .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))?,
                        ))
                    }
                };
            }

            // All attempts returned empty frames — signal the caller to fall back to GDI.
            Err(XCapError::new(
                "DXGI: all capture attempts returned an empty frame",
            ))
        }
    }
}

// ── Public entry points ───────────────────────────────────────────────────────

/// Capture a region of the monitor identified by `h_monitor` using DXGI Desktop
/// Duplication.  On HDR monitors returns [`CaptureFrame::Hdr`]; on SDR monitors
/// returns [`CaptureFrame::Sdr`].
///
/// `x`, `y`, `width`, `height` are all monitor-relative (0, 0 = top-left of monitor).
pub(super) fn capture_monitor(
    h_monitor: HMONITOR,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<CaptureFrame> {
    let session = DxgiSession::new(h_monitor)?;
    session.capture_frame(x, y, width, height)
}

/// Returns `true` only if the monitor is in Windows HDR mode AND
/// `DuplicateOutput1` with `R16G16B16A16_FLOAT` is supported by the driver.
///
/// An HDR color space alone is not sufficient — some 8-bit panels with FRC
/// report an HDR color space but the driver rejects float-format duplication.
pub(super) fn is_hdr_monitor(h_monitor: HMONITOR) -> bool {
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return false;
        };

        let mut adapter_idx = 0u32;
        loop {
            let Ok(adapter) = factory.EnumAdapters1(adapter_idx) else {
                break;
            };
            adapter_idx += 1;

            let mut output_idx = 0u32;
            loop {
                let Ok(output) = adapter.EnumOutputs(output_idx) else {
                    break;
                };
                output_idx += 1;

                let Ok(desc) = output.GetDesc() else { continue };
                if desc.Monitor != h_monitor {
                    continue;
                }

                // First check: does the color space indicate HDR at all?
                let (color_space_is_hdr, color_space_is_ycbcr) = output
                    .cast::<IDXGIOutput6>()
                    .ok()
                    .and_then(|o6| o6.GetDesc1().ok())
                    .map(|d| (is_hdr_color_space(d.ColorSpace), is_ycbcr_hdr_color_space(d.ColorSpace)))
                    .unwrap_or((false, false));

                if !color_space_is_hdr {
                    return false;
                }

                // YCbCr color spaces (AMD DP/HDMI HLG, HDR10 YCbCr) are unambiguously HDR —
                // they cannot be a false-positive from an 8-bit FRC panel.  Some AMD drivers
                // also return DXGI_ERROR_UNSUPPORTED for DuplicateOutput1 in YCbCr mode even
                // though the display is genuinely HDR, so skip the probe for these.
                if color_space_is_ycbcr {
                    return true;
                }

                // Second check (RGB HDR only): can we actually open DuplicateOutput1 with a
                // float format?  Some 8-bit panels with FRC report an RGB HDR color space but
                // the driver rejects R16G16B16A16_FLOAT with DXGI_ERROR_UNSUPPORTED.
                let Ok(output5) = output.cast::<IDXGIOutput5>() else {
                    return false;
                };

                // Create a temporary D3D11 device on this adapter to probe duplication.
                let mut d3d_device: Option<ID3D11Device> = None;
                if D3D11CreateDevice(
                    &adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut d3d_device),
                    None,
                    None,
                )
                .is_err()
                {
                    return false;
                }
                let Some(d3d_device) = d3d_device else {
                    return false;
                };
                let Ok(dxgi_device) = d3d_device.cast::<IDXGIDevice>() else {
                    return false;
                };

                let float_ok = output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R16G16B16A16_FLOAT]).is_ok();
                let pq_ok = output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R10G10B10A2_UNORM]).is_ok();
                return float_ok || pq_ok;
            }
        }
        false
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_not_found(code: HRESULT) -> bool {
    code == DXGI_ERROR_NOT_FOUND
}

/// Returns `true` for any color space that indicates the output is in an HDR mode.
///
/// Windows HDR can surface as several color spaces depending on GPU vendor and
/// connection type:
/// - `RGB_FULL_G10_NONE_P709`  – scRGB linear (NVIDIA, typical desktop HDR)
/// - `RGB_FULL_G2084_NONE_P2020` / `RGB_STUDIO_G2084_NONE_P2020` – HDR10 RGB (PQ)
/// - `YCBCR_STUDIO_G2084_*`   – HDR10 YCbCr (AMD over HDMI)
///
/// DWM always composes the desktop in scRGB linear regardless of scan-out format,
/// so `DuplicateOutput1` with `DXGI_FORMAT_R16G16B16A16_FLOAT` captures linear
/// HDR data in all of these cases.
fn is_hdr_color_space(cs: windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_TYPE) -> bool {
    cs == DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709           // scRGB linear (NVIDIA)
        || cs == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020  // RGB HDR10 PQ full
        || cs == DXGI_COLOR_SPACE_RGB_STUDIO_G2084_NONE_P2020 // RGB HDR10 PQ studio
        || is_ycbcr_hdr_color_space(cs)
}

/// Returns `true` for YCbCr HDR color spaces (AMD over HDMI/DP, HLG).
///
/// These are unambiguously HDR and some AMD drivers reject `DuplicateOutput1`
/// for these color spaces even on genuine HDR displays, so they bypass the
/// float-format probe in `is_hdr_monitor`.
fn is_ycbcr_hdr_color_space(cs: windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_TYPE) -> bool {
    cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020   // YCbCr HDR10 (AMD HDMI)
        || cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_TOPLEFT_P2020 // YCbCr HDR10 topleft
        || cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020  // HLG studio (broadcast)
        || cs == DXGI_COLOR_SPACE_YCBCR_FULL_GHLG_TOPLEFT_P2020    // HLG full range (AMD DP)
}

/// ST 2084 (PQ) inverse EOTF: normalized signal [0, 1] → linear light [0, 1]
/// relative to a 10 000 nit peak.
///
/// To convert to scRGB (1.0 = 80 nits), multiply the result by `10000.0 / 80.0`.
fn pq_eotf(np: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 32.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 128.0;
    const C3: f32 = 2392.0 / 128.0;
    let np_m2 = np.powf(1.0 / M2);
    let num = (np_m2 - C1).max(0.0);
    let den = C2 - C3 * np_m2;
    (num / den).powf(1.0 / M1)
}

/// Convert an f32 to a little-endian IEEE 754 half-precision (f16) byte pair.
fn f32_to_f16_le(v: f32) -> [u8; 2] {
    let bits = v.to_bits();
    let s = bits >> 31;
    let e = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    let m = bits & 0x007F_FFFF;
    let half: u16 = if e >= 31 {
        ((s << 15) | (0x1F << 10)) as u16 // clamp to infinity
    } else if e <= 0 {
        (s << 15) as u16 // zero (or too small for subnormal)
    } else {
        ((s << 15) | ((e as u32) << 10) | (m >> 13)) as u16
    };
    half.to_le_bytes()
}
