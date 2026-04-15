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
                DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709, DXGI_FORMAT_R16G16B16A16_FLOAT,
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
                        .map(|d| d.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)
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
            // ── Acquire the next desktop frame ────────────────────────────
            // Retry on DXGI_ERROR_WAIT_TIMEOUT: the display may not have produced
            // a frame yet (common on first acquisition or if screen is unchanged).
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

            // ── Create a CPU-readable staging texture for the ROI ─────────
            let staging_texture = {
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                desktop_texture.GetDesc(&mut src_desc);

                let mut staging_desc = src_desc;
                staging_desc.Width = width;
                staging_desc.Height = height;
                staging_desc.BindFlags = 0;
                staging_desc.MiscFlags = 0;
                staging_desc.ArraySize = 1;
                staging_desc.MipLevels = 1;
                staging_desc.SampleDesc.Count = 1;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

                let mut tex = None;
                self.d3d_device
                    .CreateTexture2D(&staging_desc, None, Some(&mut tex))?;
                tex.ok_or_else(|| XCapError::new("CreateTexture2D returned None"))?
            };

            // GPU-side copy of only the requested region.
            let region = D3D11_BOX {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
                front: 0,
                back: 1,
            };
            self.d3d_context.CopySubresourceRegion(
                Some(&staging_texture.cast()?),
                0,
                0,
                0,
                0,
                Some(&desktop_texture.cast()?),
                0,
                Some(&region),
            );

            // Release the duplication frame BEFORE mapping — mandatory to avoid lock.
            self.duplication.ReleaseFrame()?;

            // ── Map staging texture and read pixel rows ───────────────────
            // RowPitch can exceed width * bytes_per_pixel due to GPU alignment padding.
            let bytes_per_pixel = if self.is_hdr { 8usize } else { 4usize };
            let row_bytes = width as usize * bytes_per_pixel;

            let resource: windows::Win32::Graphics::Direct3D11::ID3D11Resource =
                staging_texture.cast()?;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.d3d_context
                .Map(Some(&resource), 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

            let mut raw = vec![0u8; row_bytes * height as usize];
            let src_ptr = mapped.pData as *const u8;
            for row in 0..height as usize {
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(row * mapped.RowPitch as usize),
                    raw.as_mut_ptr().add(row * row_bytes),
                    row_bytes,
                );
            }

            self.d3d_context.Unmap(Some(&resource), 0);

            // ── Produce output frame ──────────────────────────────────────
            if self.is_hdr {
                // R16G16B16A16_FLOAT is already in RGBA channel order — no swap needed.
                Ok(CaptureFrame::Hdr(HdrImage::new(width, height, raw)))
            } else {
                // B8G8R8A8_UNORM: swap B↔R before constructing RgbaImage.
                Ok(CaptureFrame::Sdr(
                    RgbaImage::from_raw(width, height, bgra_to_rgba(raw))
                        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))?,
                ))
            }
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
                    .map(|d| d.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)
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
