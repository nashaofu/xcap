use std::{
    sync::{
        Arc,
        mpsc::{Receiver, TrySendError, sync_channel},
    },
    thread,
};

use scopeguard::guard;
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
            D3D11_TEXTURE2D_DESC, ID3D11Texture2D,
        },
        Dxgi::{DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1},
        Gdi::HMONITOR,
    },
    core::Interface,
};

use super::utils::{create_d3d_device, texture_to_frame};
use crate::{
    XCapError, XCapResult,
    video_recorder::{Frame, RecorderWaker, RecorderWorker},
};

#[derive(Debug, Clone)]
pub(crate) struct ImplVideoRecorder {
    worker: Arc<RecorderWorker>,
}

impl ImplVideoRecorder {
    pub fn new(h_monitor: HMONITOR) -> XCapResult<(Self, Receiver<Frame>)> {
        let monitor_id = h_monitor.0 as usize;
        let (tx, rx) = sync_channel(2);
        let (ready_tx, ready_rx) = sync_channel(1);
        let waker = Arc::new(RecorderWaker::new());
        let thread_waker = waker.clone();
        let handle = thread::spawn(move || {
            let result = (|| -> XCapResult<()> {
                let resources = (|| -> XCapResult<_> {
                    unsafe {
                        let device = create_d3d_device(
                            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                        )?;
                        let dxgi_device = device.cast::<IDXGIDevice>()?;
                        let context = device.GetImmediateContext()?;
                        let adapter = dxgi_device.GetAdapter()?;
                        let mut index = 0;
                        loop {
                            let output = adapter.EnumOutputs(index)?;
                            index += 1;
                            if output.GetDesc()?.Monitor.0 as usize == monitor_id {
                                let duplication = output
                                    .cast::<IDXGIOutput1>()?
                                    .DuplicateOutput(&dxgi_device)?;
                                return Ok((device, context, duplication));
                            }
                        }
                    }
                })();
                let (device, context, duplication) = match resources {
                    Ok(resources) => {
                        if ready_tx.send(Ok(())).is_err() {
                            return Ok(());
                        }
                        resources
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return Ok(());
                    }
                };
                while thread_waker.wait()? {
                    unsafe {
                        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
                        let mut resource = None;
                        match duplication.AcquireNextFrame(200, &mut info, &mut resource) {
                            Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                            Err(err) => return Err(err.into()),
                            Ok(()) => {}
                        }
                        let acquired = guard((), |_| {
                            if let Err(err) = duplication.ReleaseFrame() {
                                log::error!("ReleaseFrame failed: {err}");
                            }
                        });
                        if info.LastPresentTime == 0 {
                            continue;
                        }
                        let texture = resource
                            .ok_or_else(|| XCapError::new("Missing DXGI frame"))?
                            .cast::<ID3D11Texture2D>()?;
                        let mut desc = D3D11_TEXTURE2D_DESC::default();
                        texture.GetDesc(&mut desc);
                        let frame = texture_to_frame(
                            &device,
                            &context,
                            &texture,
                            0,
                            0,
                            desc.Width,
                            desc.Height,
                        )?;
                        drop(acquired);
                        match tx.try_send(frame) {
                            Ok(()) | Err(TrySendError::Full(_)) => {}
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }
                }
                Ok(())
            })();
            thread_waker.terminate();
            if let Err(err) = result {
                log::error!("DXGI recorder failed: {err}");
            }
        });
        let worker = Arc::new(RecorderWorker {
            waker,
            handle: Some(handle),
        });
        ready_rx.recv().map_err(XCapError::new)??;
        Ok((Self { worker }, rx))
    }

    pub fn start(&self) -> XCapResult<()> {
        self.worker.waker.wake()
    }
    pub fn stop(&self) -> XCapResult<()> {
        self.worker.waker.sleep()
    }
}
