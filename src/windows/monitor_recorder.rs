use std::sync::{Arc, Condvar, Mutex};

use windows::{
    core::Interface,
    Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_HARDWARE,
        Direct3D11::{
            D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
            D3D11_SDK_VERSION,
        },
        Dxgi::{
            IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
            DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
        },
        Gdi::HMONITOR,
    },
};

use crate::{Frame, XCapError, XCapResult};

use super::dxgi::texture_to_frame;

#[derive(Debug)]
struct RecorderWaker {
    parking: Mutex<bool>,
    condvar: Condvar,
}

impl RecorderWaker {
    fn new() -> Self {
        Self {
            parking: Mutex::new(true),
            condvar: Condvar::new(),
        }
    }
    fn wake(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        *parking = false;
        self.condvar.notify_one();

        Ok(())
    }

    fn sleep(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        *parking = true;

        Ok(())
    }

    fn wait(&self) -> XCapResult<()> {
        let mut parking = self.parking.lock()?;
        while *parking {
            parking = self.condvar.wait(parking)?;
        }

        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct MonitorRecorder {
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    recorder_waker: Arc<RecorderWaker>,
}

impl MonitorRecorder {
    pub fn new(hmonitor: HMONITOR) -> XCapResult<Self> {
        unsafe {
            let mut d3d_device = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            )?;

            let d3d_device = d3d_device.ok_or(XCapError::new("Call D3D11CreateDevice failed"))?;
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

                if output_desc.Monitor == hmonitor {
                    return Ok(Self {
                        d3d_device,
                        d3d_context,
                        duplication,
                        recorder_waker: Arc::new(RecorderWaker::new()),
                    });
                }
            }
        }
    }

    pub fn on_frame<F>(&self, on_frame: F) -> XCapResult<()>
    where
        F: Fn(Frame) -> XCapResult<()> + Send + 'static,
    {
        let duplication = self.duplication.clone();
        let d3d_device = self.d3d_device.clone();
        let d3d_context = self.d3d_context.clone();
        let recorder_waker = self.recorder_waker.clone();

        loop {
            recorder_waker.wait()?;

            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;

            unsafe {
                if let Err(err) = duplication.AcquireNextFrame(200, &mut frame_info, &mut resource)
                {
                    duplication.ReleaseFrame()?;
                    if err.code() != DXGI_ERROR_WAIT_TIMEOUT {
                        break Err::<(), XCapError>(XCapError::new("DXGI_ERROR_UNSUPPORTED"));
                    }
                } else {
                    // 如何确定 AcquireNextFrame 执行成功
                    if frame_info.LastPresentTime != 0 {
                        let resource = resource.ok_or(XCapError::new("AcquireNextFrame failed"))?;
                        let source_texture = resource.cast::<ID3D11Texture2D>()?;
                        let frame = texture_to_frame(&d3d_device, &d3d_context, source_texture)?;

                        on_frame(frame)?;
                    }
                    duplication.ReleaseFrame()?;
                }
            }
        }
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
