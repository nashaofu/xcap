use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, SyncSender, sync_channel},
};
use std::time::{Duration, Instant};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Graphics::Gdi::HMONITOR,
        System::WinRT::{
            Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
    core::{Error as WindowsError, IInspectable, Interface, factory},
};

use crate::{
    XCapResult,
    video_recorder::{CaptureRegion, Frame, VideoRecorderOptions},
};

use super::wgc::{IDXGIDEVICE, get_next_frame};

#[derive(Debug)]
struct WgcRuntime {
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    closed: bool,
}

impl WgcRuntime {
    fn close(&mut self) -> XCapResult<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.session.Close()?;
        self.frame_pool.Close()?;

        Ok(())
    }
}

impl Drop for WgcRuntime {
    fn drop(&mut self) {
        self.close().unwrap_or_else(|error| {
            log::error!("Failed to close WgcRuntime: {error}");
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImplVideoRecorder {
    item: GraphicsCaptureItem,
    runtime: Arc<Mutex<Option<WgcRuntime>>>,
    tx: SyncSender<Frame>,
    region: Option<CaptureRegion>,
    interval: Duration,
    created: Instant,
}

impl ImplVideoRecorder {
    fn create_runtime(&self) -> XCapResult<WgcRuntime> {
        let item_size = self.item.Size()?;
        let (x, y, width, height) = match self.region {
            Some(region) => {
                let x = region.x.min(item_size.Width as u32 - 1);
                let y = region.y.min(item_size.Height as u32 - 1);
                let width = region.width.clamp(1, item_size.Width as u32 - x);
                let height = region.height.clamp(1, item_size.Height as u32 - y);
                (x, y, width, height)
            }
            None => (0, 0, item_size.Width as u32, item_size.Height as u32),
        };

        let device = {
            let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&*IDXGIDEVICE)? };
            inspectable.cast::<IDirect3DDevice>()?
        };

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            item_size,
        )?;

        let tx = self.tx.clone();
        let interval = self.interval;
        let created = self.created;
        let last_frame_at = Arc::new(Mutex::new(None::<Duration>));
        let callback_last_frame_at = last_frame_at.clone();

        frame_pool.FrameArrived(
            &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                move |frame_pool, _| {
                    let now = created.elapsed();
                    if let Ok(mut last) = callback_last_frame_at.lock() {
                        if last.is_some_and(|last| {
                            now.saturating_sub(last) + Duration::from_millis(1) < interval
                        }) {
                            return Ok(());
                        }
                        *last = Some(now);
                    }

                    let mut frame =
                        get_next_frame(frame_pool, x, y, width, height).map_err(|error| {
                            log::error!("wgc get_next_frame failed: {error}");
                            WindowsError::empty()
                        })?;
                    frame.timestamp = now;

                    let _ = tx.send(frame);

                    Ok(())
                },
            ),
        )?;

        let session = frame_pool.CreateCaptureSession(&self.item)?;
        // Best-effort: these may fail on older Windows builds or without capabilities
        if let Err(e) = session.SetIsBorderRequired(false) {
            log::debug!("SetIsBorderRequired(false) failed (non-fatal): {:?}", e);
        }
        if let Err(e) = session.SetIsCursorCaptureEnabled(false) {
            log::debug!(
                "SetIsCursorCaptureEnabled(false) failed (non-fatal): {:?}",
                e
            );
        }

        Ok(WgcRuntime {
            frame_pool,
            session,
            closed: false,
        })
    }

    pub fn new(
        h_monitor: HMONITOR,
        options: VideoRecorderOptions,
    ) -> XCapResult<(Self, Receiver<Frame>)> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item = unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(h_monitor)? };

        let (tx, rx) = sync_channel(0);

        let recorder = Self {
            item,
            runtime: Arc::new(Mutex::new(None)),
            tx,
            region: options.region,
            interval: Duration::from_secs_f64(1.0 / options.fps.clamp(1, 60) as f64),
            created: Instant::now(),
        };

        Ok((recorder, rx))
    }

    pub fn start(&self) -> XCapResult<()> {
        let mut runtime = self.runtime.lock()?;

        let new_runtime = self.create_runtime()?;
        new_runtime.session.StartCapture()?;
        *runtime = Some(new_runtime);

        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        let mut runtime = self.runtime.lock()?;

        if let Some(mut runtime) = runtime.take() {
            runtime.close()?;
        }

        Ok(())
    }
}
