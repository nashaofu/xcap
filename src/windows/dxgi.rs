use image::RgbaImage;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
                DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, IDXGIOutput1,
                IDXGIOutput5, IDXGIOutput6, IDXGIOutputDuplication, IDXGIResource,
            },
            Dxgi::Common::{
                DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709,
                DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                DXGI_COLOR_SPACE_RGB_STUDIO_G2084_NONE_P2020,
                DXGI_COLOR_SPACE_YCBCR_FULL_GHLG_TOPLEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_TOPLEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020,
                DXGI_FORMAT_R16G16B16A16_FLOAT,
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
    /// High Dynamic Range — scRGB linear, stored as f16 RGBA (8 bytes/pixel).
    Hdr(HdrImage),
}

// ── Internal session ──────────────────────────────────────────────────────────

struct DxgiSession {
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    /// Whether the duplication was opened with `R16G16B16A16_FLOAT` (HDR).
    is_hdr: bool,
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
                    // For HDR, try IDXGIOutput5::DuplicateOutput1 with R16G16B16A16_FLOAT.
                    // Fall back to IDXGIOutput1::DuplicateOutput (always BGRA8) if
                    // IDXGIOutput5 is unavailable or the format is rejected.
                    let (duplication, is_hdr) = if is_hdr_display {
                        match output.cast::<IDXGIOutput5>() {
                            Ok(output5) => {
                                let formats = [DXGI_FORMAT_R16G16B16A16_FLOAT];
                                match output5.DuplicateOutput1(&dxgi_device, 0, &formats) {
                                    Ok(dup) => (dup, true),
                                    Err(_) => {
                                        log::debug!(
                                            "DuplicateOutput1 rejected R16G16B16A16_FLOAT, falling back to BGRA8"
                                        );
                                        let o1 = output.cast::<IDXGIOutput1>()?;
                                        (o1.DuplicateOutput(&dxgi_device)?, false)
                                    }
                                }
                            }
                            Err(_) => {
                                let o1 = output.cast::<IDXGIOutput1>()?;
                                (o1.DuplicateOutput(&dxgi_device)?, false)
                            }
                        }
                    } else {
                        let o1 = output.cast::<IDXGIOutput1>()?;
                        (o1.DuplicateOutput(&dxgi_device)?, false)
                    };

                    return Ok(DxgiSession {
                        d3d_device,
                        d3d_context,
                        duplication,
                        is_hdr,
                    });
                }
            }
        }
    }

    /// Acquire the next desktop frame and copy the requested ROI to a CPU-accessible
    /// staging texture.  `x`, `y`, `width`, `height` are monitor-relative.
    fn capture_frame(&self, x: u32, y: u32, width: u32, height: u32) -> XCapResult<CaptureFrame> {
        unsafe {
            let bytes_per_pixel = if self.is_hdr { 8usize } else { 4usize };
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

                for _ in 0..5 {
                    match self
                        .duplication
                        .AcquireNextFrame(100, &mut frame_info, &mut resource)
                    {
                        Ok(()) => {
                            acquired = true;
                            break;
                        }
                        Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
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
                self.d3d_context
                    .Map(Some(&staging_res), 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

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
                    log::debug!(
                        "DXGI: empty frame on attempt {attempt}/{MAX_ATTEMPTS}, retrying"
                    );
                    staging_opt = Some(staging_res.cast()?);
                    continue;
                }

                // ── 8. Produce output ─────────────────────────────────────
                return if self.is_hdr {
                    // R16G16B16A16_FLOAT is already in RGBA channel order.
                    Ok(CaptureFrame::Hdr(HdrImage::new(width, height, raw)))
                } else {
                    // B8G8R8A8_UNORM: swap B↔R.
                    Ok(CaptureFrame::Sdr(
                        RgbaImage::from_raw(width, height, bgra_to_rgba(raw))
                            .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))?,
                    ))
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

/// Returns `true` if the monitor is currently in Windows HDR mode (scRGB linear).
/// Detected via `IDXGIOutput6::GetDesc1()`.
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

                return output
                    .cast::<IDXGIOutput6>()
                    .ok()
                    .and_then(|o6| o6.GetDesc1().ok())
                    .map(|d| is_hdr_color_space(d.ColorSpace))
                    .unwrap_or(false);
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
        || cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020   // YCbCr HDR10 (AMD HDMI)
        || cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_TOPLEFT_P2020 // YCbCr HDR10 topleft
        || cs == DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020  // HLG studio (broadcast)
        || cs == DXGI_COLOR_SPACE_YCBCR_FULL_GHLG_TOPLEFT_P2020    // HLG full range
}
