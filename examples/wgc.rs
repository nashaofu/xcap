use image::RgbaImage;
use std::{ops::Deref, sync::mpsc::channel, time::Duration};
use windows::{
    core::{s, w, ComObjectInterface, IInspectable, Interface, PCWSTR},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{FreeLibrary, GetLastError, HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11Resource, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
                D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING,
            },
            Dxgi::{IDXGIDevice, DXGI_ERROR_UNSUPPORTED}, Gdi::GetMonitorInfoW,
        },
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
        UI::WindowsAndMessaging::GetForegroundWindow,
    },
};
use xcap::{XCapError, XCapResult};

fn bgra_to_rgba_image(width: u32, height: u32, buffer: &mut Vec<u8>) -> XCapResult<RgbaImage> {
    for src in buffer.chunks_exact_mut(4) {
        src.swap(0, 2);
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
    session.SetIsBorderRequired(false).unwrap();

    session.StartCapture()?;

    let texture = unsafe {
        let frame = receiver.recv().unwrap();

        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let source_texture: ID3D11Texture2D = unsafe { access.GetInterface() }?;
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

fn main() {
    unsafe {
        // std::thread::sleep(Duration::from_secs(5));
        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();

        let hwnd = GetForegroundWindow();

        let s = interop.CreateForWindow(hwnd).unwrap();
        let img = d3d_capture(s).unwrap();
        img.save("screenshot.png").unwrap();

        let box_hmodule1 = BoxHModule::new(w!("GraphicsCapture.dll")).unwrap();
        let box_hmodule2 = BoxHModule::new(w!("D3D11.dll")).unwrap();

        let get_dpi_for_monitor_proc_address =
            GetProcAddress(*box_hmodule2, s!("CreateDirect3D11DeviceFromDXGIDevice")).unwrap();
        println!("{:?}", get_dpi_for_monitor_proc_address);

        let s = GetProcAddress(*box_hmodule1, s!("Create")).unwrap();
        println!("{:?}", s);

        GetMonitorInfoW(hmonitor, lpmi);
        GetMonitorTarget
    }
}

// static bool GetMonitorTarget(LPCWSTR device, DISPLAYCONFIG_TARGET_DEVICE_NAME *target)
// {
// 	bool found = false;

// 	UINT32 numPath, numMode;
// 	if (GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &numPath, &numMode) == ERROR_SUCCESS) {
// 		DISPLAYCONFIG_PATH_INFO *paths = bmalloc(numPath * sizeof(DISPLAYCONFIG_PATH_INFO));
// 		DISPLAYCONFIG_MODE_INFO *modes = bmalloc(numMode * sizeof(DISPLAYCONFIG_MODE_INFO));
// 		if (QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &numPath, paths, &numMode, modes, NULL) ==
// 		    ERROR_SUCCESS) {
// 			for (size_t i = 0; i < numPath; ++i) {
// 				const DISPLAYCONFIG_PATH_INFO *const path = &paths[i];

// 				DISPLAYCONFIG_SOURCE_DEVICE_NAME
// 				source;
// 				source.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
// 				source.header.size = sizeof(source);
// 				source.header.adapterId = path->sourceInfo.adapterId;
// 				source.header.id = path->sourceInfo.id;
// 				if (DisplayConfigGetDeviceInfo(&source.header) == ERROR_SUCCESS &&
// 				    wcscmp(device, source.viewGdiDeviceName) == 0) {
// 					target->header.type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
// 					target->header.size = sizeof(*target);
// 					target->header.adapterId = path->sourceInfo.adapterId;
// 					target->header.id = path->targetInfo.id;
// 					found = DisplayConfigGetDeviceInfo(&target->header) == ERROR_SUCCESS;
// 					break;
// 				}
// 			}
// 		}

// 		bfree(modes);
// 		bfree(paths);
// 	}

// 	return found;
// }


#[derive(Debug)]
pub struct BoxHModule(HMODULE);

impl Deref for BoxHModule {
    type Target = HMODULE;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for BoxHModule {
    fn drop(&mut self) {
        unsafe {
            if let Err(err) = FreeLibrary(self.0) {
                log::error!("FreeLibrary {:?} failed {:?}", self, err);
            }
        };
    }
}

impl BoxHModule {
    pub fn new(lib_filename: PCWSTR) -> XCapResult<Self> {
        unsafe {
            let hmodule = LoadLibraryW(lib_filename)?;

            if hmodule.is_invalid() {
                return Err(XCapError::new(format!(
                    "LoadLibraryW error {:?}",
                    GetLastError()
                )));
            }

            Ok(Self(hmodule))
        }
    }
}
