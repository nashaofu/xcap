use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, SyncSender, sync_channel},
};

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

use crate::{XCapResult, platform::wgc::get_next_frame, video_recorder::Frame};

use super::wgc::IDXGIDEVICE;

#[derive(Debug)]
struct WgcRuntime {
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl WgcRuntime {
    fn close(&self) -> XCapResult<()> {
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
}

impl ImplVideoRecorder {
    fn create_runtime(&self) -> XCapResult<WgcRuntime> {
        let item_size = self.item.Size()?;

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

        frame_pool.FrameArrived(
            &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                move |frame_pool, _| {
                    let frame = get_next_frame(
                        frame_pool,
                        0,
                        0,
                        item_size.Width as u32,
                        item_size.Height as u32,
                    )
                    .map_err(|error| {
                        log::error!("wgc process_frame_arrival failed: {error}");
                        WindowsError::empty()
                    })?;

                    let _ = tx.send(frame);

                    Ok(())
                },
            ),
        )?;

        let session = frame_pool.CreateCaptureSession(&self.item)?;
        session.SetIsBorderRequired(false)?;
        session.SetIsCursorCaptureEnabled(false)?;

        Ok(WgcRuntime {
            frame_pool,
            session,
        })
    }

    pub fn new(h_monitor: HMONITOR) -> XCapResult<(Self, Receiver<Frame>)> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item = unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(h_monitor)? };

        let (tx, rx) = sync_channel(0);

        let recorder = Self {
            item,
            runtime: Arc::new(Mutex::new(None)),
            tx,
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

        if let Some(runtime) = runtime.take() {
            runtime.close()?;
        }

        Ok(())
    }
}
