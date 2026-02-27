use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
};

use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            D3D11_CREATE_DEVICE_SINGLETHREADED, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext,
            ID3D11Resource, ID3D11Texture2D,
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
    platform::utils::create_d3d_device,
    video_recorder::{Frame, RecorderWaker},
};

use super::utils::bgra_to_rgba;

pub fn texture_to_frame(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    source_texture: &ID3D11Texture2D,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<Frame> {
    unsafe {
        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut src_desc);

        // 边界检查（防止越界）
        if x + width > src_desc.Width || y + height > src_desc.Height {
            return Err(XCapError::new("ROI out of bounds"));
        }

        let staging_texture = {
            let mut staging_desc = src_desc;
            staging_desc.Width = width;
            staging_desc.Height = height;
            staging_desc.BindFlags = 0;
            staging_desc.MiscFlags = 0;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

            let mut staging = None;
            d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            staging.ok_or(XCapError::new("CreateTexture2D failed"))?
        };

        // GPU裁剪区域
        let region = D3D11_BOX {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
            front: 0,
            back: 1,
        };

        d3d_context.CopySubresourceRegion(
            Some(&staging_texture.cast()?),
            0,
            0,
            0,
            0,
            Some(&source_texture.cast()?),
            0,
            Some(&region),
        );

        let resource: ID3D11Resource = staging_texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(
            Some(&resource.clone()),
            0,
            D3D11_MAP_READ,
            0,
            Some(&mut mapped),
        )?;

        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let src_ptr = mapped.pData as *const u8;

        for row in 0..height {
            let src_offset = (row * mapped.RowPitch) as usize;
            let dst_offset = (row * width * 4) as usize;

            let src_slice =
                std::slice::from_raw_parts(src_ptr.add(src_offset), (width * 4) as usize);

            bgra[dst_offset..dst_offset + (width * 4) as usize].copy_from_slice(src_slice);
        }

        d3d_context.Unmap(Some(&resource), 0);

        Ok(Frame::new(width, height, bgra_to_rgba(bgra.to_owned())))
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
                                let mut source_texture_desc = D3D11_TEXTURE2D_DESC::default();
                                source_texture.GetDesc(&mut source_texture_desc);

                                let frame = texture_to_frame(
                                    &d3d_device,
                                    &d3d_context,
                                    &source_texture,
                                    0,
                                    0,
                                    source_texture_desc.Width,
                                    source_texture_desc.Height,
                                )?;
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
