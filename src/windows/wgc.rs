use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, mpsc::channel},
    time::Duration,
};

use image::RgbaImage;
use scopeguard::guard;
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_TEXTURE2D_DESC, ID3D11Device,
                ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::IDXGIDevice,
            Gdi::HMONITOR,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
    core::Error as WindowsError,
    core::{IInspectable, Interface, Ref, factory},
};

use crate::{
    Frame,
    error::{XCapError, XCapResult},
};

use super::utils::{create_d3d_device, texture_to_frame};

pub(super) static ID3D11DEVICE: LazyLock<ID3D11Device> = LazyLock::new(|| {
    create_d3d_device(D3D11_CREATE_DEVICE_BGRA_SUPPORT).expect("Failed to create D3D11 device")
});

pub(super) static ID3D11DEVICE_CONTEXT: LazyLock<ID3D11DeviceContext> = LazyLock::new(|| unsafe {
    ID3D11DEVICE
        .GetImmediateContext()
        .expect("Failed to get D3D11 device context")
});
pub(super) static IDXGIDEVICE: LazyLock<IDXGIDevice> = LazyLock::new(|| {
    ID3D11DEVICE
        .cast::<IDXGIDevice>()
        .expect("Failed to cast D3D11 device to DXGI device")
});

pub(super) static MONITOR_GRAPHICS_CAPTURE_ITEM: LazyLock<
    Mutex<HashMap<usize, GraphicsCaptureItem>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) static WINDOW_GRAPHICS_CAPTURE_ITEM: LazyLock<
    Mutex<HashMap<usize, GraphicsCaptureItem>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn get_next_frame(
    frame_pool: Ref<'_, Direct3D11CaptureFramePool>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<Frame> {
    let frame_pool = frame_pool
        .as_ref()
        .ok_or(XCapError::new("Frame pool is null"))?;
    let frame = guard(frame_pool.TryGetNextFrame()?, |val| {
        val.Close().unwrap_or_else(|error| {
            log::error!("Direct3D11CaptureFrame Close failed: {:?}", error);
        });
    });

    let surface = frame.Surface()?;

    let access = surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
    let source_texture = unsafe { access.GetInterface::<ID3D11Texture2D>()? };
    let mut source_texture_desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        source_texture.GetDesc(&mut source_texture_desc);
    };

    texture_to_frame(
        &ID3D11DEVICE,
        &ID3D11DEVICE_CONTEXT,
        &source_texture,
        x,
        y,
        width,
        height,
    )
}

pub(super) fn process_frame_arrival(
    frame_pool: Ref<'_, Direct3D11CaptureFramePool>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let frame = get_next_frame(frame_pool, x, y, width, height)?;

    RgbaImage::from_raw(frame.width, frame.height, frame.raw)
        .ok_or(XCapError::new("RgbaImage::from_raw failed"))
}

pub(super) fn wgc_capture(
    item: &GraphicsCaptureItem,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let item_size = item.Size()?;
    let device = {
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&*IDXGIDEVICE)? };
        inspectable.cast::<IDirect3DDevice>()?
    };

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
    let (sender, receiver) = channel();

    frame_pool.FrameArrived(
        &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
            move |frame_pool, _| {
                let frame = process_frame_arrival(frame_pool, x, y, width, height)
                    .map_err(|_| WindowsError::empty())?;

                let _ = sender.send(frame);
                Ok(())
            }
        }),
    )?;

    let session = guard(frame_pool.CreateCaptureSession(item)?, |val| {
        val.Close().unwrap_or_else(|error| {
            log::error!("GraphicsCaptureSession Close failed: {:?}", error);
        });
    });
    // Best-effort: disable capture border and cursor.
    // SetIsBorderRequired requires Windows 11 and may fail on older builds,
    // or when the app lacks graphicsCaptureWithoutBorder capability.
    // Don't propagate errors — capture should still work with the border visible.
    if let Err(e) = session.SetIsBorderRequired(false) {
        log::debug!("SetIsBorderRequired(false) failed (non-fatal): {:?}", e);
    }
    if let Err(e) = session.SetIsCursorCaptureEnabled(false) {
        log::debug!(
            "SetIsCursorCaptureEnabled(false) failed (non-fatal): {:?}",
            e
        );
    }

    session.StartCapture()?;
    let frame = receiver.recv_timeout(Duration::from_millis(200))?;

    Ok(frame)
}

pub(super) fn capture_monitor(
    h_monitor: HMONITOR,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let mut monitor_items = MONITOR_GRAPHICS_CAPTURE_ITEM.lock()?;
    let key = h_monitor.0 as usize;
    if let Some(item) = monitor_items.get(&key) {
        wgc_capture(item, x, y, width, height)
    } else {
        let item = unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(h_monitor)? };
        monitor_items.insert(key, item.clone());
        wgc_capture(&item, x, y, width, height)
    }
}

pub(super) fn capture_window(
    hwnd: HWND,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> XCapResult<RgbaImage> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let mut window_items = WINDOW_GRAPHICS_CAPTURE_ITEM.lock()?;
    let key = hwnd.0 as usize;
    if let Some(item) = window_items.get(&key) {
        wgc_capture(item, x, y, width, height)
    } else {
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        window_items.insert(key, item.clone());
        wgc_capture(&item, x, y, width, height)
    }
}
