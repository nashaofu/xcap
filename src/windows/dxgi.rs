use std::slice;

use windows::{
    core::Interface,
    Win32::Graphics::Direct3D11::{
        ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    },
};

use crate::{Frame, XCapError, XCapResult};

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
        let buffer = slice::from_raw_parts(
            mapped.pData.cast(),
            (source_desc.Height * mapped.RowPitch) as usize,
        );

        d3d_context.Unmap(Some(&resource), 0);

        Ok(Frame::new(
            source_desc.Width,
            source_desc.Height,
            buffer.to_owned(),
        ))
    }
}
