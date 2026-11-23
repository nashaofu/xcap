//! HDR monitor capture through DXGI Desktop Duplication.
//!
//! The regular Windows screenshot path is GDI (8-bit). When the desktop is
//! composed in an HDR mode, `IDXGIOutput5::DuplicateOutput1` can hand back
//! scRGB (`R16G16B16A16_FLOAT`) or HDR10 (`R10G10B10A2_UNORM`) frames instead.
//! Everything here is best-effort: any failure means the caller keeps the GDI
//! result, so ordinary SDR desktops never touch a D3D device.

use std::time::{Duration, Instant};

use scopeguard::defer;
use windows::{
    Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
            DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
            GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
        },
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HMODULE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Resource, ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709,
                DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT,
                DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
                DXGI_MODE_ROTATION_IDENTITY, DXGI_SAMPLE_DESC,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
                DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1,
                IDXGIOutput5, IDXGIOutput6, IDXGIOutputDuplication, IDXGIResource,
            },
            Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFOEXW},
        },
    },
    core::Interface,
};

use crate::{
    HdrImage,
    error::{XCapError, XCapResult},
};

/// How long a frame may take before the display counts as asleep.
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// Uninitialised frames tolerated before the capture gives up on the driver.
const MAX_EMPTY_FRAMES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum HdrMode {
    /// `R16G16B16A16_FLOAT`: scRGB linear, already in our storage form.
    Float,
    /// `R10G10B10A2_UNORM`: ST 2084 encoded BT.2020, converted to scRGB.
    Pq,
}

#[derive(Clone)]
pub(super) struct Session {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    sdr_white_nits: f32,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("sdr_white_nits", &self.sdr_white_nits)
            .finish_non_exhaustive()
    }
}

/// Sub-rectangle of the desktop to copy, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One desktop frame copied out of the duplication surface.
struct DesktopFrame {
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    raw: Vec<u8>,
}

pub(super) fn capture_monitor(h_monitor: HMONITOR) -> XCapResult<HdrImage> {
    Session::open(h_monitor)?.capture_frame()
}

impl Session {
    pub(super) fn open(h_monitor: HMONITOR) -> XCapResult<Self> {
        let (adapter, output) = find_output(h_monitor)?;
        if !is_hdr_color_space(output_color_space(&output)) {
            return Err(XCapError::NotSupported);
        }
        let adapter: IDXGIAdapter = adapter.cast()?;
        let device = unsafe {
            let mut device = None;
            D3D11CreateDevice(
                Some(&adapter),
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
            device.ok_or_else(|| XCapError::new("D3D11CreateDevice returned no device"))?
        };
        let context = unsafe { device.GetImmediateContext() }?;
        let dxgi_device = device.cast::<IDXGIDevice>()?;

        // Prefer scRGB, fall back to HDR10 (10-bit panels / some drivers), and
        // give up to GDI when neither is accepted. The list is only a
        // preference: DXGI converts the desktop into one of the requested
        // formats, so the actual encoding is read back from the description.
        let output5 = output.cast::<IDXGIOutput5>().ok();
        let mut duplication = None;
        if let Some(output5) = output5 {
            unsafe {
                if let Ok(dup) =
                    output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R16G16B16A16_FLOAT])
                {
                    duplication = Some(dup);
                } else if let Ok(dup) =
                    output5.DuplicateOutput1(&dxgi_device, 0, &[DXGI_FORMAT_R10G10B10A2_UNORM])
                {
                    duplication = Some(dup);
                }
            }
        }
        let duplication = duplication.ok_or(XCapError::NotSupported)?;

        let desc = unsafe { duplication.GetDesc() };
        // AcquireNextFrame always returns the unrotated surface with the
        // desktop image rotated inside it, so a rotated display would not line
        // up with the selection. The GDI grab is correctly oriented.
        if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY {
            return Err(XCapError::NotSupported);
        }

        Ok(Self {
            device,
            context,
            duplication,
            sdr_white_nits: sdr_white_nits(h_monitor),
        })
    }

    fn capture_frame(&self) -> XCapResult<HdrImage> {
        let deadline = Instant::now() + FRAME_TIMEOUT;
        for attempt in 0..MAX_EMPTY_FRAMES {
            if attempt > 0 {
                // Give the driver a frame period to populate the buffer.
                std::thread::sleep(Duration::from_millis(16));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if let Some(image) = self.grab(None, remaining)? {
                return Ok(image);
            }
        }
        Err(XCapError::new("DXGI kept returning an empty desktop frame"))
    }

    /// Grab the next desktop frame as scRGB, cropped to `crop` when given.
    ///
    /// `Ok(None)` means nothing arrived within `timeout` or the driver handed
    /// back an empty buffer; video recording treats both as "try again".
    pub(super) fn grab(
        &self,
        crop: Option<Crop>,
        timeout: Duration,
    ) -> XCapResult<Option<HdrImage>> {
        let Some(frame) = self.read_frame(crop, timeout)? else {
            return Ok(None);
        };
        let image = match mode_for_format(frame.format) {
            Some(HdrMode::Float) => float_to_scrgb(&frame.raw, frame.width, frame.height),
            Some(HdrMode::Pq) => pq_to_scrgb(&frame.raw, frame.width, frame.height),
            None => {
                return Err(XCapError::new(format!(
                    "DXGI returned an unsupported desktop frame format {:?}",
                    frame.format
                )));
            }
        };
        Ok(Some(image.with_sdr_white(self.sdr_white_nits)))
    }

    /// Acquire one desktop frame as tightly packed rows.
    ///
    /// `Ok(None)` means the frame was entirely zero: some AMD drivers hand back
    /// an uninitialised buffer on the first frame after `DuplicateOutput1`.
    fn read_frame(
        &self,
        crop: Option<Crop>,
        timeout: Duration,
    ) -> XCapResult<Option<DesktopFrame>> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
            match self
                .duplication
                .AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource)
            {
                Ok(()) => {}
                Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    return Ok(None);
                }
                Err(err) => {
                    // 尝试释放当前帧，不然不能获取到下一帧数据
                    let _ = self.duplication.ReleaseFrame();
                    return Err(err.into());
                }
            }
            defer! {
                let _ = self.duplication.ReleaseFrame();
            }

            let frame = resource
                .ok_or_else(|| XCapError::new("AcquireNextFrame returned no resource"))?
                .cast::<ID3D11Texture2D>()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            frame.GetDesc(&mut desc);
            let (source_width, source_height) = (desc.Width, desc.Height);
            if source_width == 0 || source_height == 0 {
                return Err(XCapError::new("DXGI returned an empty desktop frame"));
            }
            let (x, y, width, height) = match crop {
                Some(crop) => {
                    let x = crop.x.min(source_width - 1);
                    let y = crop.y.min(source_height - 1);
                    let width = crop.width.clamp(1, source_width - x);
                    let height = crop.height.clamp(1, source_height - y);
                    (x, y, width, height)
                }
                None => (0, 0, source_width, source_height),
            };
            // The decoder follows the frame DXGI actually delivers: the
            // requested format list is only a preference, and the desktop
            // duplication description reports the display mode, not the
            // converted surface.
            let bytes_per_pixel = match mode_for_format(desc.Format) {
                Some(HdrMode::Float) => 8usize,
                Some(HdrMode::Pq) => 4usize,
                None => {
                    return Err(XCapError::new(format!(
                        "DXGI returned an unsupported desktop frame format {:?}",
                        desc.Format
                    )));
                }
            };

            let staging = self.create_staging(width, height, desc.Format)?;
            let region = D3D11_BOX {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
                front: 0,
                back: 1,
            };
            self.context.CopySubresourceRegion(
                Some(&staging.cast()?),
                0,
                0,
                0,
                0,
                Some(&frame.cast()?),
                0,
                Some(&region),
            );

            let staging_resource: ID3D11Resource = staging.cast()?;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(
                Some(&staging_resource),
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped),
            )?;
            defer! {
                self.context.Unmap(Some(&staging_resource), 0);
            }

            let row_bytes = width as usize * bytes_per_pixel;
            if mapped.pData.is_null() || (mapped.RowPitch as usize) < row_bytes {
                return Err(XCapError::new("DXGI mapped an unusable desktop frame"));
            }
            let mut raw = vec![0u8; row_bytes * height as usize];
            let src = mapped.pData as *const u8;
            for row in 0..height as usize {
                std::ptr::copy_nonoverlapping(
                    src.add(row * mapped.RowPitch as usize),
                    raw.as_mut_ptr().add(row * row_bytes),
                    row_bytes,
                );
            }
            if raw.iter().all(|&byte| byte == 0) {
                return Ok(None);
            }
            Ok(Some(DesktopFrame {
                width,
                height,
                format: desc.Format,
                raw,
            }))
        }
    }

    unsafe fn create_staging(
        &self,
        width: u32,
        height: u32,
        format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    ) -> XCapResult<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut texture = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
        texture.ok_or_else(|| XCapError::new("CreateTexture2D returned no texture"))
    }
}

/// `R16G16B16A16_FLOAT` RGBA → six-byte RGB.
fn float_to_scrgb(raw: &[u8], width: u32, height: u32) -> HdrImage {
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 6);
    for pixel in raw.as_chunks::<8>().0 {
        rgb.extend_from_slice(&pixel[..6]);
    }
    HdrImage::new(width, height, rgb, 80.0)
}

/// `R10G10B10A2_UNORM` HDR10 → scRGB.
///
/// The frame is PQ-encoded BT.2020: decode to absolute light, express it in
/// scRGB's 80-nit unit, then move to sRGB primaries so every `HdrImage` shares
/// one color space.
fn pq_to_scrgb(raw: &[u8], width: u32, height: u32) -> HdrImage {
    // 10-bit input, so a 1024-entry table replaces two `powf` calls per sample.
    let mut lut = [0f32; 1024];
    for (index, slot) in lut.iter_mut().enumerate() {
        *slot = pq_eotf(index as f32 / 1023.0) * (10_000.0 / 80.0);
    }
    let m = &BT2020_TO_SCRGB;
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 6);
    for pixel in raw.as_chunks::<4>().0 {
        let value = u32::from_le_bytes(*pixel);
        let [r, g, b] = [value & 0x3ff, (value >> 10) & 0x3ff, (value >> 20) & 0x3ff]
            .map(|channel| lut[channel as usize]);
        for linear in [
            m[0][0] * r + m[0][1] * g + m[0][2] * b,
            m[1][0] * r + m[1][1] * g + m[1][2] * b,
            m[2][0] * r + m[2][1] * g + m[2][2] * b,
        ] {
            rgb.extend_from_slice(&crate::hdr_image::f32_to_f16_bits(linear).to_le_bytes());
        }
    }
    HdrImage::new(width, height, rgb, 80.0)
}

/// linear BT.2020 → linear sRGB; the inverse of the sRGB → BT.2020 matrix the
/// PNG exporter applies.
const BT2020_TO_SCRGB: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641, -0.072_849_9],
    [-0.124_550_5, 1.132_9, -0.008_349_4],
    [-0.018_150_8, -0.100_579, 1.118_73],
];

/// ST 2084 inverse EOTF: normalized signal → linear light relative to 10 000 nits.
fn pq_eotf(signal: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 32.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 128.0;
    const C3: f32 = 2392.0 / 128.0;
    let signal_m2 = signal.powf(1.0 / M2);
    let numerator = (signal_m2 - C1).max(0.0);
    let denominator = C2 - C3 * signal_m2;
    (numerator / denominator).powf(1.0 / M1)
}

/// Luminance of SDR white on this monitor, in nits.
///
/// HDR compositing scales SDR windows to the user's SDR brightness, so SDR
/// white sits well above the 80-nit scRGB unit. `SDRWhiteLevel` reports it in
/// thousandths of 80 nits (`DISPLAYCONFIG_SDR_WHITE_LEVEL_100` = 1000).
fn sdr_white_nits(h_monitor: HMONITOR) -> f32 {
    const UNIT_NITS: f32 = 80.0;
    unsafe {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(h_monitor, (&mut info as *mut MONITORINFOEXW).cast()).as_bool() {
            return UNIT_NITS;
        }
        let Some(path) = path_for_device(&info.szDevice) else {
            return UNIT_NITS;
        };
        let mut level = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                size: std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            SDRWhiteLevel: 0,
        };
        let status = DisplayConfigGetDeviceInfo(&mut level.header);
        if status != 0 || level.SDRWhiteLevel == 0 {
            return UNIT_NITS;
        }
        UNIT_NITS * level.SDRWhiteLevel as f32 / 1000.0
    }
}

/// The active display path whose GDI device name matches `device`.
fn path_for_device(device: &[u16]) -> Option<DISPLAYCONFIG_PATH_INFO> {
    let device = trim_utf16(device);
    if device.is_empty() {
        return None;
    }
    unsafe {
        // Bound the retry: the topology must settle within ten rounds.
        for _ in 0..10 {
            let (mut paths_len, mut modes_len) = (0, 0);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_len, &mut modes_len)
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len as usize];
            let status = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths_len,
                paths.as_mut_ptr(),
                &mut modes_len,
                modes.as_mut_ptr(),
                None,
            );
            if status == ERROR_INSUFFICIENT_BUFFER {
                // The display topology changed between the two calls.
                continue;
            }
            if status != ERROR_SUCCESS {
                return None;
            }
            paths.truncate(paths_len as usize);
            return paths.into_iter().find(|path| {
                let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.sourceInfo.id,
                    },
                    viewGdiDeviceName: [0; 32],
                };
                DisplayConfigGetDeviceInfo(&mut name.header) == 0
                    && trim_utf16(&name.viewGdiDeviceName) == device
            });
        }
        None
    }
}

fn trim_utf16(value: &[u16]) -> &[u16] {
    let len = value
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(value.len());
    &value[..len]
}

fn find_output(
    h_monitor: HMONITOR,
) -> XCapResult<(IDXGIAdapter1, windows::Win32::Graphics::Dxgi::IDXGIOutput)> {
    unsafe {
        let factory = CreateDXGIFactory1::<IDXGIFactory1>()?;
        for adapter_index in 0.. {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(adapter) => adapter,
                Err(err) if err.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(err) => return Err(err.into()),
            };
            for output_index in 0.. {
                let output = match adapter.EnumOutputs(output_index) {
                    Ok(output) => output,
                    Err(err) if err.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(err) => return Err(err.into()),
                };
                if output.GetDesc()?.Monitor == h_monitor {
                    return Ok((adapter, output));
                }
            }
        }
    }
    Err(XCapError::NotSupported)
}

fn output_color_space(
    output: &windows::Win32::Graphics::Dxgi::IDXGIOutput,
) -> Option<windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_TYPE> {
    let output6 = output.cast::<IDXGIOutput6>().ok()?;
    unsafe { output6.GetDesc1().ok().map(|desc| desc.ColorSpace) }
}

fn is_hdr_color_space(
    color_space: Option<windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_TYPE>,
) -> bool {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_TYPE;
    let Some(color_space) = color_space else {
        return false;
    };
    // Only the two encodings this module decodes: scRGB (linear, full range)
    // and HDR10 (PQ-encoded full-range BT.2020). Studio-range and YCbCr
    // desktops would need a range/matrix step we do not implement, so they
    // keep the correct SDR grab instead of producing a wrong 16-bit file.
    let is_hdr = |space: DXGI_COLOR_SPACE_TYPE| color_space == space;
    is_hdr(DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)
        || is_hdr(DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020)
}

fn mode_for_format(format: DXGI_FORMAT) -> Option<HdrMode> {
    match format {
        DXGI_FORMAT_R16G16B16A16_FLOAT => Some(HdrMode::Float),
        DXGI_FORMAT_R10G10B10A2_UNORM => Some(HdrMode::Pq),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdr_image::f32_to_f16_bits;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_RGB_STUDIO_G2084_NONE_P2020,
        DXGI_FORMAT_B8G8R8A8_UNORM,
    };

    fn half(value: f32) -> [u8; 2] {
        f32_to_f16_bits(value).to_le_bytes()
    }

    fn pq_pixel(code: u32) -> [u8; 4] {
        (code | (code << 10) | (code << 20) | (3 << 30)).to_le_bytes()
    }

    #[test]
    fn only_supported_hdr_encodings_are_hdr() {
        assert!(is_hdr_color_space(Some(
            DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709
        )));
        assert!(is_hdr_color_space(Some(
            DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020
        )));
        assert!(!is_hdr_color_space(Some(
            DXGI_COLOR_SPACE_RGB_STUDIO_G2084_NONE_P2020
        )));
        assert!(!is_hdr_color_space(Some(
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709
        )));
        assert!(!is_hdr_color_space(None));
    }

    #[test]
    fn duplication_format_selects_the_decoder() {
        assert_eq!(
            mode_for_format(DXGI_FORMAT_R16G16B16A16_FLOAT),
            Some(HdrMode::Float)
        );
        assert_eq!(
            mode_for_format(DXGI_FORMAT_R10G10B10A2_UNORM),
            Some(HdrMode::Pq)
        );
        assert_eq!(mode_for_format(DXGI_FORMAT_B8G8R8A8_UNORM), None);
    }

    #[test]
    fn float_frames_keep_rgb_and_drop_alpha() {
        let mut raw = Vec::new();
        for value in [1.0f32, 2.0, 4.0, 0.5] {
            raw.extend_from_slice(&half(value));
        }
        let image = float_to_scrgb(&raw, 1, 1);
        assert_eq!(image.pixel_f32(0, 0), [1.0, 2.0, 4.0]);
        assert_eq!(image.reference_white_nits(), 80.0);
    }

    #[test]
    fn pq_frames_decode_to_absolute_nits() {
        // 10-bit code 1023 is 10 000 nits: 125 scRGB units at 80 nits.
        let white = pq_to_scrgb(&pq_pixel(1023), 1, 1);
        for channel in white.pixel_f32(0, 0) {
            assert!((channel - 125.0).abs() < 0.1, "{channel}");
        }
        // Neutral input stays neutral through BT.2020 -> sRGB.
        let mid = pq_to_scrgb(&pq_pixel(512), 1, 1);
        let [r, g, b] = mid.pixel_f32(0, 0);
        assert!((r - g).abs() < 0.01 && (g - b).abs() < 0.01, "{r} {g} {b}");
        assert!(r > 0.0 && r < 125.0);
    }
}
