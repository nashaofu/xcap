use std::{slice, time::Instant};

use image::RgbaImage;
use windows::{
    core::Interface,
    Win32::Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
            D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_11_1,
        },
        Direct3D11::{
            D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
            D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG,
            D3D11_CREATE_DEVICE_FLAG, D3D11_CREATE_DEVICE_SINGLETHREADED, D3D11_MAPPED_SUBRESOURCE,
            D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        },
        Dxgi::{
            IDXGIDevice, IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
            DXGI_ERROR_UNSUPPORTED, DXGI_OUTDUPL_FRAME_INFO,
        },
    },
};
use xcap::{XCapError, XCapResult};

fn bgra_to_rgba_image(width: u32, height: u32, mut buffer: Vec<u8>) -> XCapResult<RgbaImage> {
    for src in buffer.chunks_exact_mut(4) {
        src.swap(0, 2);
        if src[3] == 0 {
            src[3] = 255;
        }
    }

    let d = RgbaImage::from_raw(width as u32, height as u32, buffer).unwrap();
    Ok(d)
}

fn create_d3d_device_with_type(
    driver_type: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    device: *mut Option<ID3D11Device>,
) -> windows::core::Result<()> {
    let mut feature_level = D3D_FEATURE_LEVEL::default();
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            None,
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(device),
            Some(&mut feature_level),
            None,
        )?;
        println!("feature_level {:?}", feature_level);
        Ok(())
    }
}

fn create_d3d_device() -> ID3D11Device {
    let mut device = None;
    let mut result = create_d3d_device_with_type(
        D3D_DRIVER_TYPE_HARDWARE,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
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
    result.unwrap();
    device.unwrap()
}

pub fn d3d_capture(
    output: IDXGIOutput,
    d3d_device: &ID3D11Device,
    dxgi_device: &IDXGIDevice,
    d3d_context: &ID3D11DeviceContext,
) -> windows::core::Result<RgbaImage> {
    unsafe {
        let output1 = output.cast::<IDXGIOutput1>().unwrap();
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(dxgi_device)?;
        let mut resource: Option<IDXGIResource> = None;

        let mut f = 0;
        let s = loop {
            let start = Instant::now();

            if let Err(err) = duplication.AcquireNextFrame(200, &mut frame_info, &mut resource) {
                println!("err {:?}", err);
                duplication.ReleaseFrame().unwrap();
                continue;
            }

            // 如何确定AcquireNextFrame 执行成功
            if frame_info.LastPresentTime == 0 {
                duplication.ReleaseFrame().unwrap();
                continue;
            } else {
                println!("i {}", f);

                let d = resource.clone().unwrap().cast::<ID3D11Texture2D>()?;

                let s = texture_to_rgba_image(d3d_device, d3d_context, d).unwrap();
                s.save(format!("images/a-{}.png", f)).unwrap();
                f += 1;
                duplication.ReleaseFrame().unwrap();
                println!("运行耗时: {:?}", start.elapsed());
                // break s;
            }
        };

        Ok(s)
    }
}

fn texture_to_rgba_image(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    source_texture: ID3D11Texture2D,
) -> XCapResult<RgbaImage> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        source_texture.GetDesc(&mut desc);
        desc.BindFlags = 0;
        desc.MiscFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

        let copy_texture = {
            let mut texture = None;
            d3d_device.CreateTexture2D(&desc, None, Some(&mut texture))?;
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
        let buffer: Vec<u8> = slice::from_raw_parts(
            mapped.pData.cast(),
            (desc.Height * mapped.RowPitch) as usize,
        )
        .to_vec();

        d3d_context.Unmap(Some(&resource), 0);

        bgra_to_rgba_image(desc.Width, desc.Height, buffer)
    }
}

fn main() {
    let d3d_device = create_d3d_device();
    let d3d_context = unsafe { d3d_device.GetImmediateContext().unwrap() };

    let dxgi_device: IDXGIDevice = d3d_device.cast().unwrap();
    let adapter = unsafe { dxgi_device.GetAdapter().unwrap() };

    let mut output_index = 0;
    loop {
        if let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
            output_index += 1;
            let img = d3d_capture(output, &d3d_device, &dxgi_device, &d3d_context).unwrap();
        } else {
            break;
        }
    }
}
