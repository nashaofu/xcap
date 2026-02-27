use std::{sync::mpsc::channel, time::Duration};

use image::RgbaImage;
use scopeguard::guard;
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_TEXTURE2D_DESC, ID3D11Device,
                ID3D11Texture2D,
            },
            Dxgi::IDXGIDevice,
            Gdi::HMONITOR,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
    core::{IInspectable, Interface, factory},
};

use crate::{
    error::{XCapError, XCapResult},
    platform::{impl_video_recorder::texture_to_frame, utils::create_d3d_device},
};

fn create_direct3d_device(d3d_device: &ID3D11Device) -> windows::core::Result<IDirect3DDevice> {
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    inspectable.cast()
}

pub fn wgc_capture(
    item: GraphicsCaptureItem,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    if !GraphicsCaptureSession::IsSupported()? {
        return Err(XCapError::new("GraphicsCaptureSession is not supported"));
    }

    let item_size = item.Size()?;

    let d3d_device = create_d3d_device(D3D11_CREATE_DEVICE_BGRA_SUPPORT)?;
    let d3d_context = unsafe { d3d_device.GetImmediateContext()? };
    let device = create_direct3d_device(&d3d_device)?;

    let frame_pool = guard(
        Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            item_size,
        )?,
        |val| {
            val.Close().unwrap_or_else(|error| {
                log::error!("Direct3D11CaptureFramePool Close failed: {:?}", error);
            });
        },
    );

    let session = guard(frame_pool.CreateCaptureSession(&item)?, |val| {
        val.Close().unwrap_or_else(|error| {
            log::error!("GraphicsCaptureSession Close failed: {:?}", error);
        });
    });
    session.SetIsBorderRequired(false)?;
    session.SetIsCursorCaptureEnabled(false)?;

    let (sender, receiver) = channel();
    frame_pool.FrameArrived(
        &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
            move |frame_pool, _| {
                let frame_pool = frame_pool.as_ref().unwrap();
                let frame = frame_pool.TryGetNextFrame()?;
                let _ = sender.send(frame);
                Ok(())
            }
        }),
    )?;
    session.StartCapture()?;
    let frame = receiver.recv_timeout(Duration::from_millis(1000)).unwrap();
    session.Close()?;
    frame_pool.Close()?;
    let surface = frame.Surface()?;
    frame.Close()?;

    let access = surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
    let source_texture = unsafe { access.GetInterface::<ID3D11Texture2D>()? };
    let mut source_texture_desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        source_texture.GetDesc(&mut source_texture_desc);
    };

    let frame = texture_to_frame(
        &d3d_device,
        &d3d_context,
        &source_texture,
        x,
        y,
        width,
        height,
    )?;

    RgbaImage::from_raw(frame.width, frame.height, frame.raw)
        .ok_or(XCapError::new("RgbaImage::from_raw failed"))
}

pub fn capture_monitor(
    hmonitor: HMONITOR,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(hmonitor)? };
    wgc_capture(item, x, y, width, height)
}

pub fn capture_window(
    hwnd: HWND,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
    wgc_capture(item, x, y, width, height)
}
