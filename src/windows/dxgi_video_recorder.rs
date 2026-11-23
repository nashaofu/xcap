use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
            D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        },
        Dxgi::{
            DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1,
            IDXGIOutputDuplication, IDXGIResource,
        },
        Gdi::HMONITOR,
    },
    core::Interface,
};

use crate::{
    XCapError, XCapResult,
    video_recorder::{CaptureRegion, DynamicRange, Frame, RecorderWaker, VideoRecorderOptions},
};

use super::{
    dxgi,
    utils::{create_d3d_device, texture_to_frame},
};

/// Where frames come from: ordinary BGRA duplication, or an HDR duplication
/// session that decodes scRGB. The HDR variant is only chosen when the display
/// is composited in HDR mode, so `Frame::hdr` presence still decides what the
/// encoder sees.
#[derive(Debug, Clone)]
enum Backend {
    Sdr {
        d3d_device: ID3D11Device,
        d3d_context: ID3D11DeviceContext,
        duplication: IDXGIOutputDuplication,
    },
    Hdr(dxgi::Session),
}

#[derive(Debug, Clone)]
pub(crate) struct ImplVideoRecorder {
    backend: Backend,
    recorder_waker: Arc<RecorderWaker>,
    tx: SyncSender<Frame>,
    region: Option<CaptureRegion>,
    /// Minimum time between delivered frames, from the frame-rate cap.
    interval: Duration,
    created: Instant,
}

impl ImplVideoRecorder {
    pub fn new(
        h_monitor: HMONITOR,
        options: VideoRecorderOptions,
    ) -> XCapResult<(Self, Receiver<Frame>)> {
        // `VideoRecorderOptions::output_size` is a macOS compositor hint with
        // no DXGI equivalent: `OutputSize::Logical` is deliberately ignored,
        // this backend copies a sub-region at native resolution, and the
        // caller downscales the delivered frames itself.
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

        let (tx, rx) = sync_channel(0);
        let recorder = Self {
            backend,
            recorder_waker: Arc::new(RecorderWaker::new()),
            tx,
            region: options.region,
            interval: Duration::from_secs_f64(1.0 / options.fps.clamp(1, 60) as f64),
            created: Instant::now(),
        };
        recorder.on_frame()?;
        Ok((recorder, rx))
    }

    fn open_sdr(h_monitor: HMONITOR) -> XCapResult<Backend> {
        unsafe {
            let d3d_device = create_d3d_device(
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
            )?;
            let dxgi_device = d3d_device.cast::<IDXGIDevice>()?;
            let d3d_context = d3d_device.GetImmediateContext()?;

            let adapter = dxgi_device.GetAdapter()?;

            let mut output_index = 0;
            loop {
                let output = adapter.EnumOutputs(output_index)?;
                output_index += 1;
                let output_desc = output.GetDesc()?;

                let output1 = output.cast::<IDXGIOutput1>()?;
                let duplication = output1.DuplicateOutput(&dxgi_device)?;

                if output_desc.Monitor == h_monitor {
                    return Ok(Backend::Sdr {
                        d3d_device,
                        d3d_context,
                        duplication,
                    });
                }
            }
        }
    }

    pub fn on_frame(&self) -> XCapResult<()> {
        let recorder_waker = self.recorder_waker.clone();
        let tx = self.tx.clone();
        let region = self.region;
        let interval = self.interval;
        let created = self.created;
        let backend = self.backend.clone();

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
                    recorder_waker,
                    tx,
                    region,
                    interval,
                    created,
                ),
                Backend::Hdr(session) => {
                    run_hdr(session, recorder_waker, tx, region, interval, created)
                }
            };
            if let Err(err) = result {
                log::warn!("the DXGI video recorder stopped: {err}");
            }
        });

        Ok(())
    }

    pub fn start(&self) -> XCapResult<()> {
        self.recorder_waker.wake()?;

        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.recorder_waker.sleep()?;

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
    loop {
        recorder_waker.wait()?;

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        unsafe {
            match duplication.AcquireNextFrame(200, &mut frame_info, &mut resource) {
                Err(err) => {
                    // 尝试释放当前帧，不然不能获取到下一帧数据
                    let _ = duplication.ReleaseFrame();
                    if err.code() != DXGI_ERROR_WAIT_TIMEOUT {
                        return Err(XCapError::new("DXGI_ERROR_UNSUPPORTED"));
                    }
                }
                _ => {
                    // 如何确定 AcquireNextFrame 执行成功
                    if frame_info.LastPresentTime != 0 {
                        let resource = resource.ok_or(XCapError::new("AcquireNextFrame failed"))?;
                        let source_texture = resource.cast::<ID3D11Texture2D>()?;
                        let mut source_texture_desc = D3D11_TEXTURE2D_DESC::default();
                        source_texture.GetDesc(&mut source_texture_desc);

                        let now = created.elapsed();
                        if let Some(last) = last_frame_at {
                            // One millisecond of tolerance keeps a 60 Hz
                            // source from skipping every other frame at a
                            // 60 fps cap.
                            if now.saturating_sub(last) + Duration::from_millis(1) < interval {
                                let _ = duplication.ReleaseFrame();
                                continue;
                            }
                        }

                        let (x, y, width, height) = match region {
                            Some(region) => {
                                let x = region.x.min(source_texture_desc.Width - 1);
                                let y = region.y.min(source_texture_desc.Height - 1);
                                let width = region.width.clamp(1, source_texture_desc.Width - x);
                                let height = region.height.clamp(1, source_texture_desc.Height - y);
                                (x, y, width, height)
                            }
                            None => (0, 0, source_texture_desc.Width, source_texture_desc.Height),
                        };

                        let mut frame = texture_to_frame(
                            &d3d_device,
                            &d3d_context,
                            &source_texture,
                            x,
                            y,
                            width,
                            height,
                        )?;
                        // Stamp capture time, not delivery time: the
                        // conversion above can take longer than a frame.
                        frame.timestamp = now;
                        last_frame_at = Some(now);
                        let _ = tx.send(frame);
                    }

                    // 最后释放帧，不然获取不到当前帧的数据
                    duplication.ReleaseFrame()?;
                }
            }
        }
    }
}

/// HDR frames arrive as scRGB and are already cropped by the D3D copy, so the
/// only work between the desktop and the encoder is decoding half floats.
fn run_hdr(
    session: dxgi::Session,
    recorder_waker: Arc<RecorderWaker>,
    tx: SyncSender<Frame>,
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
        recorder_waker.wait()?;

        // Throttle before acquiring: converting a 4K HDR frame only to throw
        // it away costs far more than the frame check does.
        if let Some(last) = last_frame_at {
            let wait = interval.saturating_sub(created.elapsed().saturating_sub(last));
            if !wait.is_zero() {
                thread::sleep(wait);
            }
        }
        let captured_at = created.elapsed();
        let Some(image) = session.grab(crop, Duration::from_millis(200))? else {
            continue;
        };
        last_frame_at = Some(captured_at);
        let width = image.width();
        let height = image.height();
        let _ = tx.send(Frame::new_hdr(width, height, image, captured_at));
    }
}
