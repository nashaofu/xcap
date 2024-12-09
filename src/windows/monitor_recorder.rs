use std::thread;

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
            IDXGIDevice, IDXGIOutput, IDXGIOutput1, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT,
            DXGI_OUTDUPL_FRAME_INFO,
        },
        Gdi::HMONITOR,
    },
};

use crate::{Frame, XCapError, XCapResult};

use super::dxgi::texture_to_frame;

pub struct MonitorRecorder {
    d3d_device: ID3D11Device,
    dxgi_device: IDXGIDevice,
    d3d_context: ID3D11DeviceContext,
    output: IDXGIOutput,
}

impl MonitorRecorder {
    pub fn from_monitor(hmonitor: HMONITOR) -> XCapResult<Self> {
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
                if output_desc.Monitor == hmonitor {
                    return Ok(Self {
                        d3d_device,
                        dxgi_device,
                        d3d_context,
                        output,
                    });
                }
            }
        }
    }

    pub fn start<F>(&self, on_frame: F) -> XCapResult<()>
    where
        F: Fn(Frame) -> XCapResult<()> + Send + 'static,
    {
        let d = unsafe {
            let output1 = self.output.cast::<IDXGIOutput1>()?;

            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let duplication = output1.DuplicateOutput(&self.dxgi_device)?;

            let d3d_device = self.d3d_device.clone();
            let d3d_context = self.d3d_context.clone();

            thread::spawn(move || {
                loop {
                    let mut resource: Option<IDXGIResource> = None;
                    if let Err(err) =
                        duplication.AcquireNextFrame(200, &mut frame_info, &mut resource)
                    {
                        duplication.ReleaseFrame()?;
                        if err.code() != DXGI_ERROR_WAIT_TIMEOUT {
                            break Err::<(), XCapError>(XCapError::new("DXGI_ERROR_UNSUPPORTED"));
                        }
                        continue;
                    }

                    // 如何确定AcquireNextFrame 执行成功
                    if frame_info.LastPresentTime == 0 {
                        duplication.ReleaseFrame()?;
                        continue;
                    } else {
                        let resource = resource.ok_or(XCapError::new("AcquireNextFrame failed"))?;
                        let source_texture = resource.cast::<ID3D11Texture2D>()?;
                        let frame = texture_to_frame(&d3d_device, &d3d_context, source_texture)?;

                        on_frame(frame)?;

                        duplication.ReleaseFrame()?;
                    }
                }
            })
        };

        d.join().map_err(|_| XCapError::new("join failed"))?
    }
}
