//! Output metadata and the wlroots screenshot fallback.
//!
//! Screencopy v2 guarantees shared-memory buffers. Binding at most v2 avoids
//! requiring GBM/EGL or a DMA-BUF-only path on compositors advertising v3.

use std::{
    fs::File,
    io::{ErrorKind, Read},
    os::fd::AsFd,
    time::{Duration, Instant},
};

use image::{RgbaImage, imageops};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{MemfdFlags, memfd_create},
};
use scopeguard::defer;
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop,
    protocol::{
        wl_buffer::WlBuffer,
        wl_callback::{self, WlCallback},
        wl_output::{self, Transform, WlOutput},
        wl_registry::{self, WlRegistry},
        wl_shm::{Format, WlShm},
        wl_shm_pool::WlShmPool,
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

use crate::{XCapError, XCapResult};

fn wayland_error(error: impl ToString) -> XCapError {
    XCapError::WaylandError(error.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Region {
    fn intersection(self, other: Self) -> Option<Self> {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let bottom = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));
        (left < right && top < bottom).then(|| Self {
            x: left as i32,
            y: top as i32,
            width: (right - left) as i32,
            height: (bottom - top) as i32,
        })
    }
}

#[derive(Clone)]
struct Output {
    proxy: WlOutput,
    global_name: u32,
    region: Region,
    pixel_width: i32,
    pixel_height: i32,
    transform: Transform,
}

impl Output {
    fn scale(&self) -> Option<f64> {
        if self.region.width <= 0
            || self.region.height <= 0
            || self.pixel_width <= 0
            || self.pixel_height <= 0
        {
            return None;
        }
        let height = match self.transform {
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
                self.pixel_width
            }
            _ => self.pixel_height,
        };
        Some(f64::from(height) / f64::from(self.region.height))
    }
}

#[derive(Clone, Copy)]
struct BufferInfo {
    format: Format,
    width: u32,
    height: u32,
    stride: u32,
}

#[derive(Default)]
struct State {
    outputs: Vec<Output>,
    output_manager: Option<ZxdgOutputManagerV1>,
    shm: Option<WlShm>,
    screencopy: Option<ZwlrScreencopyManagerV1>,
    synced: bool,
    buffer: Option<BufferInfo>,
    ready: bool,
    y_invert: bool,
    failure: Option<String>,
}

struct Session {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    deadline: Instant,
}

impl Session {
    fn new() -> XCapResult<Self> {
        let connection = Connection::connect_to_env().map_err(wayland_error)?;
        let queue = connection.new_event_queue();
        let mut session = Self {
            connection,
            queue,
            state: State::default(),
            deadline: Instant::now() + Duration::from_secs(10),
        };
        session
            .connection
            .display()
            .get_registry(&session.queue.handle(), ());
        session.sync()?;
        // Output bindings made while dispatching the registry need another sync.
        session.sync()?;
        let manager = session
            .state
            .output_manager
            .as_ref()
            .ok_or_else(|| wayland_error("Compositor does not support xdg-output"))?;
        for output in &session.state.outputs {
            manager.get_xdg_output(&output.proxy, &session.queue.handle(), output.global_name);
        }
        session.sync()?;
        if !session
            .state
            .outputs
            .iter()
            .any(|output| output.scale().is_some())
        {
            return Err(wayland_error("No outputs with valid logical dimensions"));
        }
        Ok(session)
    }

    fn sync(&mut self) -> XCapResult<()> {
        self.state.synced = false;
        self.connection.display().sync(&self.queue.handle(), ());
        self.wait_until(|state| state.synced)
    }

    fn wait_until(&mut self, done: impl Fn(&State) -> bool) -> XCapResult<()> {
        loop {
            if Instant::now() >= self.deadline {
                return Err(wayland_error("Timed out waiting for the compositor"));
            }
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(wayland_error)?;
            if let Some(error) = &self.state.failure {
                return Err(wayland_error(error));
            }
            if done(&self.state) {
                return Ok(());
            }
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            // A full socket must be polled for writability before retrying flush.
            let needs_flush = match self.connection.flush() {
                Ok(()) => false,
                Err(wayland_client::backend::WaylandError::Io(error))
                    if error.kind() == ErrorKind::WouldBlock =>
                {
                    true
                }
                Err(error) => return Err(wayland_error(error)),
            };
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: remaining.as_secs() as _,
                tv_nsec: remaining.subsec_nanos() as _,
            };
            let backend = self.connection.backend();
            let events = if needs_flush {
                PollFlags::IN | PollFlags::OUT
            } else {
                PollFlags::IN
            };
            let mut fds = [PollFd::from_borrowed_fd(backend.poll_fd(), events)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err(wayland_error("Timed out waiting for the compositor")),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(wayland_error(error)),
            }
            if fds[0]
                .revents()
                .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
            {
                return Err(wayland_error("Wayland connection closed"));
            }
            if fds[0].revents().contains(PollFlags::IN) {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(error))
                        if error.kind() == ErrorKind::WouldBlock => {}
                    Err(error) => return Err(wayland_error(error)),
                }
            }
        }
    }

    fn capture_output(&mut self, output: &Output, region: Region) -> XCapResult<RgbaImage> {
        self.state.buffer = None;
        self.state.ready = false;
        self.state.y_invert = false;
        let manager = self
            .state
            .screencopy
            .as_ref()
            .ok_or_else(|| wayland_error("Compositor does not support wlr-screencopy"))?;
        let frame = manager.capture_output_region(
            0,
            &output.proxy,
            region.x - output.region.x,
            region.y - output.region.y,
            region.width,
            region.height,
            &self.queue.handle(),
            (),
        );
        defer! { frame.destroy(); }
        self.wait_until(|state| state.buffer.is_some())?;
        let info = self
            .state
            .buffer
            .ok_or_else(|| wayland_error("No shared-memory buffer"))?;
        let length = info.byte_length()?;
        let mut file = File::from(
            memfd_create("xcap-screencopy", MemfdFlags::CLOEXEC).map_err(wayland_error)?,
        );
        file.set_len(length as u64)?;
        let shm = self
            .state
            .shm
            .as_ref()
            .ok_or_else(|| wayland_error("Compositor does not support wl_shm"))?;
        let pool = shm.create_pool(file.as_fd(), length as i32, &self.queue.handle(), ());
        let buffer = pool.create_buffer(
            0,
            info.width as i32,
            info.height as i32,
            info.stride as i32,
            info.format,
            &self.queue.handle(),
            (),
        );
        pool.destroy();
        defer! { buffer.destroy(); }
        frame.copy(&buffer);
        self.wait_until(|state| state.ready)?;
        // Read only after ready; no unsafe shared-memory mapping is necessary.
        let mut data = allocate_bytes(length)?;
        file.read_exact(&mut data)?;
        let image = decode_buffer(info, &data, self.state.y_invert)?;
        Ok(transform_image(image, output.transform))
    }
}

pub(super) fn scale_factor() -> XCapResult<f32> {
    let session = Session::new()?;
    Ok(session
        .state
        .outputs
        .iter()
        .filter_map(Output::scale)
        .reduce(f64::max)
        .ok_or_else(|| wayland_error("No output scale"))? as f32)
}

/// Capture a region in the compositor's global logical coordinate space.
pub(super) fn capture(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    if width <= 0 || height <= 0 {
        return Err(XCapError::InvalidCaptureRegion(
            "Dimensions must be positive".into(),
        ));
    }
    let mut session = Session::new()?;
    let requested = Region {
        x,
        y,
        width,
        height,
    };
    let outputs: Vec<_> = session
        .state
        .outputs
        .iter()
        .filter_map(|output| {
            output.scale()?;
            output
                .region
                .intersection(requested)
                .map(|region| (output.clone(), region))
        })
        .collect();
    let scale = outputs
        .iter()
        .filter_map(|(output, _)| output.scale())
        .fold(1.0, f64::max);
    if outputs.is_empty() {
        return Err(wayland_error(
            "Capture region does not intersect any output",
        ));
    }
    let result_width = scaled_dimension(width, scale)?;
    let result_height = scaled_dimension(height, scale)?;
    let mut result = empty_image(result_width, result_height)?;
    for (output, region) in outputs {
        let image = session.capture_output(&output, region)?;
        // Derive both edges from the same origin so fractional scales do not
        // introduce seams between adjacent outputs.
        let left = scaled_offset(i64::from(region.x) - i64::from(x), scale)?;
        let top = scaled_offset(i64::from(region.y) - i64::from(y), scale)?;
        let right = scaled_offset(
            i64::from(region.x) - i64::from(x) + i64::from(region.width),
            scale,
        )?;
        let bottom = scaled_offset(
            i64::from(region.y) - i64::from(y) + i64::from(region.height),
            scale,
        )?;
        let image = if image.dimensions() == (right - left, bottom - top) {
            image
        } else {
            imageops::resize(
                &image,
                right - left,
                bottom - top,
                imageops::FilterType::Lanczos3,
            )
        };
        imageops::replace(&mut result, &image, i64::from(left), i64::from(top));
    }
    Ok(result)
}

fn scaled_dimension(value: i32, scale: f64) -> XCapResult<u32> {
    scaled_offset(i64::from(value), scale)
}

fn scaled_offset(value: i64, scale: f64) -> XCapResult<u32> {
    let value = (value as f64 * scale).round();
    if !value.is_finite() || value < 0.0 || value > i32::MAX as f64 {
        return Err(wayland_error(
            "Scaled capture dimensions exceed supported bounds",
        ));
    }
    Ok(value as u32)
}

fn allocate_bytes(length: usize) -> XCapResult<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(wayland_error)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn empty_image(width: u32, height: u32) -> XCapResult<RgbaImage> {
    let length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| wayland_error("Image dimensions overflow"))?;
    if width == 0 || height == 0 || length > i32::MAX as u64 {
        return Err(wayland_error("Invalid or oversized image dimensions"));
    }
    RgbaImage::from_raw(width, height, allocate_bytes(length as usize)?)
        .ok_or_else(|| wayland_error("Invalid RGBA buffer"))
}

impl BufferInfo {
    fn bytes_per_pixel(self) -> XCapResult<usize> {
        match self.format {
            Format::Xrgb8888
            | Format::Argb8888
            | Format::Xbgr8888
            | Format::Abgr8888
            | Format::Xrgb2101010
            | Format::Argb2101010
            | Format::Xbgr2101010
            | Format::Abgr2101010 => Ok(4),
            Format::Rgb888 | Format::Bgr888 => Ok(3),
            _ => Err(wayland_error(format!(
                "Unsupported wl_shm pixel format: {:?}",
                self.format
            ))),
        }
    }

    fn byte_length(self) -> XCapResult<usize> {
        let row_bytes = u64::from(self.width) * self.bytes_per_pixel()? as u64;
        let length = u64::from(self.stride) * u64::from(self.height);
        if self.width == 0
            || self.height == 0
            || self.width > i32::MAX as u32
            || self.height > i32::MAX as u32
            || row_bytes > u64::from(self.stride)
            || length > i32::MAX as u64
        {
            return Err(wayland_error(
                "Invalid shared-memory buffer dimensions or stride",
            ));
        }
        Ok(length as usize)
    }
}

fn decode_buffer(info: BufferInfo, data: &[u8], y_invert: bool) -> XCapResult<RgbaImage> {
    if data.len() < info.byte_length()? {
        return Err(wayland_error("Truncated shared-memory buffer"));
    }
    let bpp = info.bytes_per_pixel()?;
    let mut image = empty_image(info.width, info.height)?;
    for y in 0..info.height {
        let source_y = if y_invert { info.height - y - 1 } else { y };
        let row = source_y as usize * info.stride as usize;
        for x in 0..info.width {
            let start = row + x as usize * bpp;
            let bytes = &data[start..start + bpp];
            let (r, g, b, a) = if bpp == 3 {
                let rgb_order = (info.format == Format::Bgr888) == cfg!(target_endian = "little");
                if rgb_order {
                    (bytes[0], bytes[1], bytes[2], 255)
                } else {
                    (bytes[2], bytes[1], bytes[0], 255)
                }
            } else {
                let pixel = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                match info.format {
                    Format::Xrgb8888 | Format::Argb8888 => (
                        (pixel >> 16) as u8,
                        (pixel >> 8) as u8,
                        pixel as u8,
                        if info.format == Format::Argb8888 {
                            (pixel >> 24) as u8
                        } else {
                            255
                        },
                    ),
                    Format::Xbgr8888 | Format::Abgr8888 => (
                        pixel as u8,
                        (pixel >> 8) as u8,
                        (pixel >> 16) as u8,
                        if info.format == Format::Abgr8888 {
                            (pixel >> 24) as u8
                        } else {
                            255
                        },
                    ),
                    _ => {
                        let low = ((pixel & 1023) * 255 / 1023) as u8;
                        let middle = (((pixel >> 10) & 1023) * 255 / 1023) as u8;
                        let high = (((pixel >> 20) & 1023) * 255 / 1023) as u8;
                        let alpha =
                            if matches!(info.format, Format::Argb2101010 | Format::Abgr2101010) {
                                (pixel >> 30) as u8 * 85
                            } else {
                                255
                            };
                        if matches!(info.format, Format::Xbgr2101010 | Format::Abgr2101010) {
                            (low, middle, high, alpha)
                        } else {
                            (high, middle, low, alpha)
                        }
                    }
                }
            };
            // wl_shm uses premultiplied alpha; image::RgbaImage uses straight alpha.
            let straight = |channel: u8| {
                if a == 255 {
                    channel
                } else if a == 0 {
                    0
                } else {
                    (u16::from(channel) * 255 / u16::from(a)).min(255) as u8
                }
            };
            image.put_pixel(
                x,
                y,
                image::Rgba([straight(r), straight(g), straight(b), a]),
            );
        }
    }
    Ok(image)
}

fn transform_image(image: RgbaImage, transform: Transform) -> RgbaImage {
    let image = if matches!(
        transform,
        Transform::Flipped | Transform::Flipped90 | Transform::Flipped180 | Transform::Flipped270
    ) {
        imageops::flip_horizontal(&image)
    } else {
        image
    };
    match transform {
        Transform::_90 | Transform::Flipped90 => imageops::rotate90(&image),
        Transform::_180 | Transform::Flipped180 => imageops::rotate180(&image),
        Transform::_270 | Transform::Flipped270 => imageops::rotate270(&image),
        _ => image,
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_output" if version >= 2 => state.outputs.push(Output {
                    proxy: registry.bind(name, version.min(4), qh, ()),
                    global_name: name,
                    region: Region {
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    },
                    pixel_width: 0,
                    pixel_height: 0,
                    transform: Transform::Normal,
                }),
                "zxdg_output_manager_v1" => {
                    state.output_manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_screencopy_manager_v1" => {
                    state.screencopy = Some(registry.bind(name, version.min(2), qh, ()));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|output| output.global_name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state
            .outputs
            .iter_mut()
            .find(|output| output.proxy == *proxy)
        else {
            return;
        };
        match event {
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => {
                output.pixel_width = width;
                output.pixel_height = height;
            }
            wl_output::Event::Geometry {
                transform: WEnum::Value(transform),
                ..
            } => {
                output.transform = transform;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state
            .outputs
            .iter_mut()
            .find(|output| output.global_name == *name)
        else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.region.x = x;
                output.region.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.region.width = width;
                output.region.height = height;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } => {
                state.buffer = Some(BufferInfo {
                    format,
                    width,
                    height,
                    stride,
                });
            }
            zwlr_screencopy_frame_v1::Event::Buffer { .. } => {
                state.failure = Some("Unknown shared-memory pixel format".into());
            }
            zwlr_screencopy_frame_v1::Event::Flags {
                flags: WEnum::Value(flags),
            } => {
                state.y_invert = flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => state.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => {
                state.failure = Some("Screencopy failed".into())
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore ZxdgOutputManagerV1);
delegate_noop!(State: ignore ZwlrScreencopyManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_native_pixel_formats_and_alpha() {
        for (format, packed, expected) in [
            (Format::Xrgb8888, 0x00123456u32, [0x12, 0x34, 0x56, 255]),
            (Format::Xbgr8888, 0x00563412, [0x12, 0x34, 0x56, 255]),
            (Format::Argb8888, 0x80800000, [255, 0, 0, 128]),
            (Format::Abgr8888, 0x80000080, [255, 0, 0, 128]),
            (Format::Xrgb2101010, 1023 << 20, [255, 0, 0, 255]),
            (Format::Xbgr2101010, 1023, [255, 0, 0, 255]),
            (
                Format::Argb2101010,
                (3 << 30) | (1023 << 20),
                [255, 0, 0, 255],
            ),
            (Format::Abgr2101010, (3 << 30) | 1023, [255, 0, 0, 255]),
        ] {
            let info = BufferInfo {
                format,
                width: 1,
                height: 1,
                stride: 4,
            };
            let image = decode_buffer(info, &packed.to_ne_bytes(), false).unwrap();
            assert_eq!(image.get_pixel(0, 0).0, expected, "{format:?}");
        }
    }

    #[test]
    fn decodes_24_bit_formats() {
        for (format, packed) in [(Format::Rgb888, 0x123456u32), (Format::Bgr888, 0x563412)] {
            let packed = packed.to_ne_bytes();
            let bytes = if cfg!(target_endian = "little") {
                &packed[..3]
            } else {
                &packed[1..]
            };
            let info = BufferInfo {
                format,
                width: 1,
                height: 1,
                stride: 3,
            };
            assert_eq!(
                decode_buffer(info, bytes, false).unwrap().get_pixel(0, 0).0,
                [0x12, 0x34, 0x56, 255],
            );
        }
    }

    #[test]
    fn skips_row_padding_and_applies_y_inversion() {
        let info = BufferInfo {
            format: Format::Xrgb8888,
            width: 1,
            height: 2,
            stride: 8,
        };
        let mut data = Vec::new();
        data.extend_from_slice(&0x00ff0000u32.to_ne_bytes());
        data.extend_from_slice(&[99; 4]);
        data.extend_from_slice(&0x000000ffu32.to_ne_bytes());
        data.extend_from_slice(&[99; 4]);
        let image = decode_buffer(info, &data, true).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(0, 1).0, [255, 0, 0, 255]);
        assert!(decode_buffer(info, &data[..15], false).is_err());
    }

    #[test]
    fn rejects_invalid_buffer_dimensions_and_formats() {
        for (format, width, height, stride) in [
            (Format::Xrgb8888, 0, 1, 4),
            (Format::Xrgb8888, 2, 1, 7),
            (Format::Xrgb8888, 1, u32::MAX, 4),
            (Format::Xrgb8888, 1, 2, u32::MAX),
            (Format::C8, 1, 1, 1),
        ] {
            assert!(
                BufferInfo {
                    format,
                    width,
                    height,
                    stride
                }
                .byte_length()
                .is_err()
            );
        }
    }

    #[test]
    fn transforms_all_output_orientations() {
        let image =
            RgbaImage::from_fn(2, 3, |x, y| image::Rgba([(y * 2 + x + 1) as u8, 0, 0, 255]));
        for (transform, dimensions, expected) in [
            (Transform::Normal, (2, 3), [1, 2, 3, 4, 5, 6]),
            (Transform::_90, (3, 2), [5, 3, 1, 6, 4, 2]),
            (Transform::_180, (2, 3), [6, 5, 4, 3, 2, 1]),
            (Transform::_270, (3, 2), [2, 4, 6, 1, 3, 5]),
            (Transform::Flipped, (2, 3), [2, 1, 4, 3, 6, 5]),
            (Transform::Flipped90, (3, 2), [6, 4, 2, 5, 3, 1]),
            (Transform::Flipped180, (2, 3), [5, 6, 3, 4, 1, 2]),
            (Transform::Flipped270, (3, 2), [1, 3, 5, 2, 4, 6]),
        ] {
            let transformed = transform_image(image.clone(), transform);
            assert_eq!(transformed.dimensions(), dimensions);
            assert_eq!(
                transformed
                    .pixels()
                    .map(|pixel| pixel[0])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn clips_negative_coordinates_without_integer_overflow() {
        let output = Region {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let request = Region {
            x: -10,
            y: 20,
            width: 20,
            height: 30,
        };
        assert_eq!(
            output.intersection(request),
            Some(Region {
                x: -10,
                y: 20,
                width: 10,
                height: 30
            })
        );
        assert!(
            output
                .intersection(Region {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                })
                .is_none()
        );
        let edge = Region {
            x: i32::MAX - 5,
            y: 0,
            width: 10,
            height: 1,
        };
        assert_eq!(edge.intersection(edge), Some(edge));
    }

    #[test]
    fn validates_scaled_dimensions() {
        assert_eq!(scaled_dimension(400, 1.5).unwrap(), 600);
        assert_eq!(scaled_offset(3, 1.5).unwrap(), 5);
        assert!(scaled_dimension(i32::MAX, 2.0).is_err());
        assert!(scaled_dimension(1, f64::NAN).is_err());
        assert!(scaled_dimension(1, f64::INFINITY).is_err());
        assert!(empty_image(0, 1).is_err());
        assert!(empty_image(u32::MAX, u32::MAX).is_err());
    }
}
