use std::{
    slice,
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
};

use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_SINGLETHREADED, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
                ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
            },
            Dxgi::{
                DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1,
                IDXGIOutputDuplication, IDXGIResource,
            },
            Gdi::HMONITOR,
        },
    },
    core::Interface,
};

use crate::{
    XCapError, XCapResult,
    video_recorder::{Frame, RecorderWaker},
};

use super::utils::bgra_to_rgba;

pub fn texture_to_frame(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    source_texture: ID3D11Texture2D,
) -> XCapResult<Frame> {
    unsafe {
        let mut source_desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut source_desc);
        source_desc.BindFlags = 0;
        source_desc.MiscFlags = 0;
        source_desc.Usage = D3D11_USAGE_STAGING;
        source_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

        let copy_texture = {
            let mut texture = None;
            d3d_device.CreateTexture2D(&source_desc, None, Some(&mut texture))?;
            texture.ok_or(XCapError::new("CreateTexture2D failed"))?
        };

        d3d_context.CopyResource(Some(&copy_texture.cast()?), Some(&source_texture.cast()?));

        let resource: ID3D11Resource = copy_texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(
            Some(&resource.clone()),
            0,
            D3D11_MAP_READ,
            0,
            Some(&mut mapped),
        )?;

        // Get a slice of bytes
        let bgra = slice::from_raw_parts(
            mapped.pData.cast(),
            (source_desc.Height * mapped.RowPitch) as usize,
        );

        d3d_context.Unmap(Some(&resource), 0);

        Ok(Frame::new(
            source_desc.Width,
            source_desc.Height,
            bgra_to_rgba(bgra.to_owned()),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ImplVideoRecorder {
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    recorder_waker: Arc<RecorderWaker>,
    tx: SyncSender<Frame>,
}

impl ImplVideoRecorder {
    pub fn new(h_monitor: HMONITOR) -> XCapResult<(Self, Receiver<Frame>)> {
        unsafe {
            let mut d3d_device = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
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

                if output_desc.Monitor == h_monitor {
                    let (tx, sx) = sync_channel(0);
                    let s = Self {
                        d3d_device,
                        d3d_context,
                        duplication,
                        recorder_waker: Arc::new(RecorderWaker::new()),
                        tx,
                    };
                    s.on_frame()?;
                    return Ok((s, sx));
                }
            }
        }
    }

    pub fn on_frame(&self) -> XCapResult<()> {
        let duplication = self.duplication.clone();
        let d3d_device = self.d3d_device.clone();
        let d3d_context = self.d3d_context.clone();
        let recorder_waker = self.recorder_waker.clone();
        let tx = self.tx.clone();

        thread::spawn(move || {
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
                                break Err::<(), XCapError>(XCapError::new(
                                    "DXGI_ERROR_UNSUPPORTED",
                                ));
                            }
                        }
                        _ => {
                            // 如何确定 AcquireNextFrame 执行成功
                            if frame_info.LastPresentTime != 0 {
                                let resource =
                                    resource.ok_or(XCapError::new("AcquireNextFrame failed"))?;
                                let source_texture = resource.cast::<ID3D11Texture2D>()?;
                                let frame =
                                    texture_to_frame(&d3d_device, &d3d_context, source_texture)?;
                                let _ = tx.send(frame);
                            }

                            // 最后释放帧，不然获取不到当前帧的数据
                            duplication.ReleaseFrame()?;
                        }
                    }
                }
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
