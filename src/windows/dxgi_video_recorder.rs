use std::{
    sync::{
        Arc,
        atomic::AtomicUsize,
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use scopeguard::defer;
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
            D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        },
        Dxgi::{
            Common::{
                DXGI_MODE_ROTATION, DXGI_MODE_ROTATION_ROTATE90, DXGI_MODE_ROTATION_ROTATE180,
                DXGI_MODE_ROTATION_ROTATE270,
            },
            DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter, IDXGIDevice,
            IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
        },
        Gdi::HMONITOR,
    },
    core::Interface,
};

use crate::{
    XCapError, XCapResult,
    video_recorder::{
        CaptureRegion, DynamicRange, Frame, QueuePermit, RecorderWaker, VideoRecorderOptions,
    },
};

use super::{
    dxgi,
    utils::{create_d3d_device, texture_to_frame},
};

const FRAME_QUEUE_CAPACITY: usize = 4;

/// Where frames come from: ordinary BGRA duplication, or an HDR duplication
/// session that decodes scRGB. The HDR variant is only chosen when the display
/// is composited in HDR mode, so checking `Frame::hdr` decides the range.
#[derive(Debug)]
enum Backend {
    Sdr {
        d3d_device: ID3D11Device,
        d3d_context: ID3D11DeviceContext,
        duplication: IDXGIOutputDuplication,
    },
    Hdr(dxgi::Session),
}

#[derive(Debug)]
struct WakerGuard(Arc<RecorderWaker>);

impl Drop for WakerGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImplVideoRecorder {
    recorder_waker: Arc<RecorderWaker>,
    _waker_guard: Arc<WakerGuard>,
}

impl ImplVideoRecorder {
    pub fn new(
        h_monitor: HMONITOR,
        options: VideoRecorderOptions,
    ) -> XCapResult<(Self, Receiver<Frame>)> {
        // `VideoRecorderOptions::output_size` is a macOS compositor hint with
        // no DXGI equivalent: `OutputSize::Logical` is deliberately ignored,
        // and this backend copies a sub-region at native resolution.
        let backend = if options.dynamic_range == DynamicRange::Hdr {
            match dxgi::Session::open(h_monitor) {
                Ok(session) => Backend::Hdr(session),
                Err(err) => {
                    // An SDR desktop, a rotated display or a driver without
                    // DuplicateOutput1 support all land here; the recording
                    // still works, just without extended range.
                    log::info!("HDR desktop duplication unavailable, recording SDR: {err}");
                    Self::open_sdr(h_monitor)?
                }
            }
        } else {
            Self::open_sdr(h_monitor)?
        };

        let (tx, rx) = sync_channel(FRAME_QUEUE_CAPACITY);
        let queue_depth = Arc::new(AtomicUsize::new(0));
        let recorder_waker = Arc::new(RecorderWaker::new());
        let region = options.region;
        let interval = Duration::from_secs_f64(1.0 / options.fps.clamp(1, 60) as f64);
        let created = Instant::now();

        let worker_waker = recorder_waker.clone();
        thread::spawn(move || {
            let result = match backend {
                Backend::Sdr {
                    d3d_device,
                    d3d_context,
                    duplication,
                } => run_sdr(
                    d3d_device,
                    d3d_context,
                    duplication,
                    worker_waker,
                    tx,
                    region,
                    interval,
                    created,
                ),
                Backend::Hdr(session) => run_hdr(
                    session,
                    worker_waker,
                    tx,
                    queue_depth,
                    region,
                    interval,
                    created,
                ),
            };
            if let Err(err) = result {
                log::warn!("the DXGI video recorder stopped: {err}");
            }
        });

        let recorder = Self {
            _waker_guard: Arc::new(WakerGuard(recorder_waker.clone())),
            recorder_waker,
        };
        Ok((recorder, rx))
    }

    fn open_sdr(h_monitor: HMONITOR) -> XCapResult<Backend> {
        let (adapter, output) = dxgi::find_output(h_monitor)?;
        let adapter = adapter.cast::<IDXGIAdapter>()?;
        let d3d_device = create_d3d_device(
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
            Some(&adapter),
        )?;
        let dxgi_device = d3d_device.cast::<IDXGIDevice>()?;
        let d3d_context = unsafe { d3d_device.GetImmediateContext()? };
        let output1 = output.cast::<IDXGIOutput1>()?;
        let duplication = unsafe { output1.DuplicateOutput(&dxgi_device)? };

        Ok(Backend::Sdr {
            d3d_device,
            d3d_context,
            duplication,
        })
    }

    pub fn start(&self) -> XCapResult<()> {
        if self.recorder_waker.is_stopped() {
            return Err(XCapError::new("DXGI video recorder has already stopped"));
        }
        self.recorder_waker.wake()?;

        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.recorder_waker.shutdown()?;

        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn run_sdr(
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    recorder_waker: Arc<RecorderWaker>,
    tx: SyncSender<Frame>,
    region: Option<CaptureRegion>,
    interval: Duration,
    created: Instant,
) -> XCapResult<()> {
    let mut last_frame_at: Option<Duration> = None;
    let rotation = unsafe { duplication.GetDesc().Rotation };
    loop {
        if !recorder_waker.wait_active()? {
            return Ok(());
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        unsafe {
            match duplication.AcquireNextFrame(200, &mut frame_info, &mut resource) {
                Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                Err(err) => {
                    // Release any held frame before propagating the failure.
                    let _ = duplication.ReleaseFrame();
                    return Err(err.into());
                }
                Ok(()) => {}
            }
            defer! {
                let _ = duplication.ReleaseFrame();
            }

            if frame_info.LastPresentTime == 0 {
                continue;
            }
            let resource = resource.ok_or_else(|| XCapError::new("AcquireNextFrame failed"))?;
            let source_texture = resource.cast::<ID3D11Texture2D>()?;
            let mut source_texture_desc = D3D11_TEXTURE2D_DESC::default();
            source_texture.GetDesc(&mut source_texture_desc);

            let now = created.elapsed();
            if let Some(last) = last_frame_at {
                // One millisecond of tolerance keeps a 60 Hz source from
                // skipping every other frame at a 60 fps cap.
                if now.saturating_sub(last) + Duration::from_millis(1) < interval {
                    continue;
                }
            }

            let (x, y, width, height) = sdr_crop(
                region,
                source_texture_desc.Width,
                source_texture_desc.Height,
                rotation,
            )?;

            let mut frame = orient_frame(
                texture_to_frame(
                    &d3d_device,
                    &d3d_context,
                    &source_texture,
                    x,
                    y,
                    width,
                    height,
                )?,
                rotation,
            );
            // Stamp capture time, not delivery time: the conversion above can
            // take longer than a frame.
            frame.timestamp = now;
            last_frame_at = Some(now);
            match tx.try_send(frame) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    recorder_waker.shutdown()?;
                    return Ok(());
                }
            }
        }
    }
}

/// Map a screen-space region into DXGI's unrotated duplication surface.
fn sdr_crop(
    region: Option<CaptureRegion>,
    source_width: u32,
    source_height: u32,
    rotation: DXGI_MODE_ROTATION,
) -> XCapResult<(u32, u32, u32, u32)> {
    if source_width == 0 || source_height == 0 {
        return Err(XCapError::new("DXGI returned an empty desktop frame"));
    }
    let Some(region) = region else {
        return Ok((0, 0, source_width, source_height));
    };
    let quarter_turn = matches!(
        rotation,
        DXGI_MODE_ROTATION_ROTATE90 | DXGI_MODE_ROTATION_ROTATE270
    );
    let (screen_width, screen_height) = if quarter_turn {
        (source_height, source_width)
    } else {
        (source_width, source_height)
    };
    let x = region.x.min(screen_width - 1);
    let y = region.y.min(screen_height - 1);
    let width = region.width.clamp(1, screen_width - x);
    let height = region.height.clamp(1, screen_height - y);

    Ok(match rotation {
        DXGI_MODE_ROTATION_ROTATE90 => (y, screen_width - x - width, height, width),
        DXGI_MODE_ROTATION_ROTATE180 => (
            screen_width - x - width,
            screen_height - y - height,
            width,
            height,
        ),
        DXGI_MODE_ROTATION_ROTATE270 => (screen_height - y - height, x, height, width),
        _ => (x, y, width, height),
    })
}

/// Rotate a cropped surface back to the display's screen orientation.
fn orient_frame(mut frame: Frame, rotation: DXGI_MODE_ROTATION) -> Frame {
    let (width, height) = match rotation {
        DXGI_MODE_ROTATION_ROTATE90 | DXGI_MODE_ROTATION_ROTATE270 => (frame.height, frame.width),
        DXGI_MODE_ROTATION_ROTATE180 => (frame.width, frame.height),
        _ => return frame,
    };
    let source_width = frame.width as usize;
    let source_height = frame.height as usize;
    let mut raw = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let (source_x, source_y) = match rotation {
                DXGI_MODE_ROTATION_ROTATE90 => (y, source_height - 1 - x),
                DXGI_MODE_ROTATION_ROTATE180 => (source_width - 1 - x, source_height - 1 - y),
                DXGI_MODE_ROTATION_ROTATE270 => (source_width - 1 - y, x),
                _ => unreachable!(),
            };
            let source = (source_y * source_width + source_x) * 4;
            let target = (y * width as usize + x) * 4;
            raw[target..target + 4].copy_from_slice(&frame.raw[source..source + 4]);
        }
    }
    frame.raw = raw;
    frame.width = width;
    frame.height = height;
    frame
}

/// HDR frames arrive as scRGB and are already cropped by the D3D copy, so the
/// only work between the desktop and the encoder is decoding half floats.
fn run_hdr(
    session: dxgi::Session,
    recorder_waker: Arc<RecorderWaker>,
    tx: SyncSender<Frame>,
    queue_depth: Arc<AtomicUsize>,
    region: Option<CaptureRegion>,
    interval: Duration,
    created: Instant,
) -> XCapResult<()> {
    let crop = region.map(|region| dxgi::Crop {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
    });
    let mut last_frame_at: Option<Duration> = None;
    loop {
        if !recorder_waker.wait_active()? {
            return Ok(());
        }

        // Throttle before acquiring: converting a 4K HDR frame only to throw
        // it away costs far more than the frame check does.
        if let Some(last) = last_frame_at {
            let wait = interval.saturating_sub(created.elapsed().saturating_sub(last));
            if !recorder_waker.wait_timeout(wait)? {
                return Ok(());
            }
        }
        let Some(permit) = QueuePermit::acquire(queue_depth.clone(), FRAME_QUEUE_CAPACITY) else {
            // The receiver has not made room yet. Skip before DXGI grab and
            // HDR decode rather than paying for a frame that would be dropped.
            if !recorder_waker.wait_timeout(interval)? {
                return Ok(());
            }
            continue;
        };
        let Some((image, acquired_at)) = session.grab(crop, Duration::from_millis(200))? else {
            continue;
        };
        let captured_at = acquired_at.saturating_duration_since(created);
        last_frame_at = Some(captured_at);
        let width = image.width();
        let height = image.height();
        let frame = Frame::new_hdr(width, height, image, captured_at).with_queue_permit(permit);
        match tx.try_send(frame) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                recorder_waker.shutdown()?;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_ROTATION_IDENTITY;

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 100,
            y: 50,
            width: 200,
            height: 100,
        }
    }

    #[test]
    fn screen_regions_map_into_unrotated_surfaces() {
        assert_eq!(
            sdr_crop(Some(region()), 1920, 1080, DXGI_MODE_ROTATION_IDENTITY).unwrap(),
            (100, 50, 200, 100)
        );
        assert_eq!(
            sdr_crop(Some(region()), 1920, 1080, DXGI_MODE_ROTATION_ROTATE90).unwrap(),
            (50, 780, 100, 200)
        );
        assert_eq!(
            sdr_crop(Some(region()), 1920, 1080, DXGI_MODE_ROTATION_ROTATE180).unwrap(),
            (1620, 930, 200, 100)
        );
        assert_eq!(
            sdr_crop(Some(region()), 1920, 1080, DXGI_MODE_ROTATION_ROTATE270).unwrap(),
            (1770, 100, 100, 200)
        );
    }

    #[test]
    fn cropped_frames_return_to_screen_orientation() {
        let frame = Frame::new(
            2,
            3,
            (0..6).flat_map(|pixel| [pixel as u8, 0, 0, 255]).collect(),
        );
        let values = |frame: Frame| {
            frame
                .raw
                .chunks_exact(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>()
        };
        assert_eq!(
            values(orient_frame(frame.clone(), DXGI_MODE_ROTATION_ROTATE90)),
            [4, 2, 0, 5, 3, 1]
        );
        assert_eq!(
            values(orient_frame(frame.clone(), DXGI_MODE_ROTATION_ROTATE180)),
            [5, 4, 3, 2, 1, 0]
        );
        assert_eq!(
            values(orient_frame(frame, DXGI_MODE_ROTATION_ROTATE270)),
            [1, 3, 5, 0, 2, 4]
        );
    }

    #[test]
    fn sdr_crop_and_orient_frame_round_trip_for_all_rotations() {
        let full = Frame::new(
            2,
            3,
            (0..6).flat_map(|pixel| [pixel as u8, 0, 0, 255]).collect(),
        );
        let crop_surface = |(sx, sy, sw, sh): (u32, u32, u32, u32)| {
            let mut raw = Vec::with_capacity((sw * sh * 4) as usize);
            for y in sy..sy + sh {
                for x in sx..sx + sw {
                    let at = ((y * full.width + x) * 4) as usize;
                    raw.extend_from_slice(&full.raw[at..at + 4]);
                }
            }
            Frame::new(sw, sh, raw)
        };
        for rotation in [
            DXGI_MODE_ROTATION_IDENTITY,
            DXGI_MODE_ROTATION_ROTATE90,
            DXGI_MODE_ROTATION_ROTATE180,
            DXGI_MODE_ROTATION_ROTATE270,
        ] {
            let oriented_full = orient_frame(full.clone(), rotation);
            for sy in 0..oriented_full.height {
                for sx in 0..oriented_full.width {
                    let region = CaptureRegion {
                        x: sx,
                        y: sy,
                        width: 1,
                        height: 1,
                    };
                    let rect = sdr_crop(Some(region), full.width, full.height, rotation).unwrap();
                    let oriented_crop = orient_frame(crop_surface(rect), rotation);
                    let expected = ((sy * oriented_full.width + sx) * 4) as usize;
                    assert_eq!(
                        oriented_crop.raw,
                        oriented_full.raw[expected..expected + 4],
                        "mismatch at ({sx}, {sy}) for {rotation:?}"
                    );
                }
            }
        }
    }
}
