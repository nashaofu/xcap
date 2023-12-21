use image::RgbaImage;
use std::sync::mpsc::channel;
use windows::{
    core::{ComInterface, IInspectable, Interface},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11Resource, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
                D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING,
            },
            Dxgi::{IDXGIDevice, DXGI_ERROR_UNSUPPORTED},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
};

use crate::{XCapError, XCapResult};

use super::{impl_monitor::ImplMonitor, impl_window::ImplWindow, utils::get_os_major_version};

fn bgra_to_rgba_image(width: u32, height: u32, buffer: &mut Vec<u8>) -> XCapResult<RgbaImage> {
    for src in buffer.chunks_exact_mut(4) {
        src.swap(0, 2);
        // fix https://github.com/nashaofu/xcap/issues/92#issuecomment-1910014951
        if src[3] == 0 && get_os_major_version() < 8 {
            src[3] = 255;
        }
    }

    RgbaImage::from_raw(width as u32, height as u32, buffer.to_owned())
        .ok_or_else(|| XCapError::new("RgbaImage::from_raw failed"))
}

fn create_d3d_device_with_type(
    driver_type: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    device: *mut Option<ID3D11Device>,
) -> windows::core::Result<()> {
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            None,
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(device),
            None,
            None,
        )
    }
}

fn create_d3d_device() -> XCapResult<ID3D11Device> {
    let mut device = None;
    let mut result = create_d3d_device_with_type(
        D3D_DRIVER_TYPE_HARDWARE,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        &mut device,
    );
    if let Err(error) = &result {
        if error.code() == DXGI_ERROR_UNSUPPORTED {
            result = create_d3d_device_with_type(
                D3D_DRIVER_TYPE_WARP,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                &mut device,
            );
        }
    }
    result?;
    Ok(device.unwrap())
}

fn create_direct3d_device(d3d_device: &ID3D11Device) -> windows::core::Result<IDirect3DDevice> {
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    inspectable.cast()
}

fn get_d3d_interface_from_object<S: Interface + ComInterface, R: Interface + ComInterface>(
    object: &S,
) -> XCapResult<R> {
    let access: IDirect3DDxgiInterfaceAccess = object.cast()?;
    let object = unsafe { access.GetInterface::<R>()? };
    Ok(object)
}

impl TryFrom<&ImplMonitor> for GraphicsCaptureItem {
    type Error = windows::core::Error;
    fn try_from(impl_monitor: &ImplMonitor) -> Result<Self, Self::Error> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        unsafe { interop.CreateForMonitor(impl_monitor.hmonitor) }
    }
}

impl TryFrom<&ImplWindow> for GraphicsCaptureItem {
    type Error = windows::core::Error;
    fn try_from(impl_window: &ImplWindow) -> Result<Self, Self::Error> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        unsafe { interop.CreateForWindow(impl_window.hwnd) }
    }
}

#[allow(unused)]
pub fn d3d_capture(item: GraphicsCaptureItem) -> XCapResult<RgbaImage> {
    let item_size = item.Size()?;

    let d3d_device = create_d3d_device()?;
    let d3d_context = unsafe { d3d_device.GetImmediateContext()? };
    let device = create_direct3d_device(&d3d_device)?;
    let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        item_size,
    )?;
    let session = frame_pool.CreateCaptureSession(&item)?;

    let (sender, receiver) = channel();
    frame_pool.FrameArrived(
        &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
            move |frame_pool, _| {
                let frame_pool = frame_pool.as_ref().unwrap();
                let frame = frame_pool.TryGetNextFrame()?;
                sender.send(frame).unwrap();
                Ok(())
            }
        }),
    )?;
    session.StartCapture()?;

    let texture = unsafe {
        let frame = receiver.recv().unwrap();

        let source_texture: ID3D11Texture2D = get_d3d_interface_from_object(&frame.Surface()?)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut desc);
        desc.BindFlags = 0;
        desc.MiscFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let copy_texture = {
            let mut texture = None;
            d3d_device.CreateTexture2D(&desc, None, Some(&mut texture))?;
            texture.unwrap()
        };

        d3d_context.CopyResource(Some(&copy_texture.cast()?), Some(&source_texture.cast()?));

        session.Close()?;
        frame_pool.Close()?;

        copy_texture
    };

    let bits = unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc as *mut _);

        let resource: ID3D11Resource = texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(
            Some(&resource.clone()),
            0,
            D3D11_MAP_READ,
            0,
            Some(&mut mapped),
        )?;

        // Get a slice of bytes
        let slice: &[u8] = {
            std::slice::from_raw_parts(
                mapped.pData as *const _,
                (desc.Height * mapped.RowPitch) as usize,
            )
        };

        let bytes_per_pixel = 4;
        let mut bits = vec![0u8; (desc.Width * desc.Height * bytes_per_pixel) as usize];
        for row in 0..desc.Height {
            let data_begin = (row * (desc.Width * bytes_per_pixel)) as usize;
            let data_end = ((row + 1) * (desc.Width * bytes_per_pixel)) as usize;
            let slice_begin = (row * mapped.RowPitch) as usize;
            let slice_end = slice_begin + (desc.Width * bytes_per_pixel) as usize;
            bits[data_begin..data_end].copy_from_slice(&slice[slice_begin..slice_end]);
        }

        d3d_context.Unmap(Some(&resource), 0);

        bgra_to_rgba_image(desc.Width, desc.Height, &mut bits)?
    };

    Ok(bits)
}
