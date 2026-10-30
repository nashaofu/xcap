use std::{
    fmt,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::Duration,
};

use ashpd::desktop::screencast::{Screencast, SelectSourcesOptions, SourceType};

use pipewire::{
    channel,
    context::ContextRc,
    keys::{MEDIA_CATEGORY, MEDIA_ROLE, MEDIA_TYPE},
    main_loop::MainLoopRc,
    properties,
    spa::{
        param::{
            ParamType,
            format::{FormatProperties, MediaSubtype, MediaType},
            format_utils,
            video::{VideoFormat, VideoInfoRaw},
        },
        pod::{self, Pod, serialize::PodSerializer},
        utils::{Direction, Fraction, Rectangle, SpaTypes},
    },
    stream::{StreamFlags, StreamRc, StreamState},
};

use super::impl_monitor::ImplMonitor;
use crate::{XCapError, XCapResult, video_recorder::Frame};

#[derive(Clone)]
pub struct WaylandVideoRecorder {
    monitor: ImplMonitor,
    shared: Arc<RecorderRuntime>,
}

struct RecorderRuntime {
    is_running: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    active_sender: Mutex<channel::Sender<RecorderCommand>>,
    handle: Option<thread::JoinHandle<()>>,
}

enum RecorderCommand {
    Active(bool, SyncSender<XCapResult<()>>),
    Shutdown,
}

impl Drop for RecorderRuntime {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Release);
        let sender = self
            .active_sender
            .get_mut()
            .unwrap_or_else(|err| err.into_inner());
        let _ = sender.send(RecorderCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                log::error!("PipeWire recorder worker panicked");
            }
        }
    }
}

impl fmt::Debug for WaylandVideoRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaylandVideoRecorder")
            .field("monitor", &self.monitor)
            .field("is_running", &self.shared.is_running)
            .finish()
    }
}

struct ListenerUserData {
    pub format: VideoInfoRaw,
}

fn decode_frame(
    data: &[u8],
    offset: usize,
    chunk_size: usize,
    stride: i32,
    format: VideoFormat,
    width: u32,
    height: u32,
) -> XCapResult<Frame> {
    let pixel_size = match format {
        VideoFormat::RGB => 3,
        VideoFormat::RGBA | VideoFormat::RGBx | VideoFormat::BGRx => 4,
        _ => return Err(XCapError::new("Unsupported PipeWire pixel format")),
    };
    let width_usize = width as usize;
    let height_usize = height as usize;
    let row_len = width_usize
        .checked_mul(pixel_size)
        .ok_or_else(|| XCapError::new("PipeWire row size overflow"))?;
    let stride_len = stride.unsigned_abs() as usize;
    let required = height_usize
        .checked_sub(1)
        .and_then(|rows| rows.checked_mul(stride_len))
        .and_then(|bytes| bytes.checked_add(row_len))
        .ok_or_else(|| XCapError::new("Invalid PipeWire frame size"))?;
    if data.is_empty() {
        return Err(XCapError::new("Empty PipeWire buffer"));
    }
    let offset = offset % data.len();
    let bytes = offset
        .checked_add(chunk_size)
        .and_then(|end| data.get(offset..end))
        .ok_or_else(|| XCapError::new("PipeWire chunk exceeds buffer"))?;
    if width == 0 || height == 0 || stride_len < row_len || required > bytes.len() {
        return Err(XCapError::new("Invalid PipeWire row layout"));
    }
    let raw_len = width_usize
        .checked_mul(height_usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&len| len <= isize::MAX as usize)
        .ok_or_else(|| XCapError::new("PipeWire frame size overflow"))?;
    let mut raw = vec![0; raw_len];
    for row in 0..height_usize {
        let source_row = if stride < 0 {
            height_usize - 1 - row
        } else {
            row
        };
        let src = &bytes[source_row * stride_len..source_row * stride_len + row_len];
        let dst = &mut raw[row * width_usize * 4..(row + 1) * width_usize * 4];
        for (src, dst) in src.chunks_exact(pixel_size).zip(dst.chunks_exact_mut(4)) {
            dst[..3].copy_from_slice(&src[..3]);
            if format == VideoFormat::BGRx {
                dst.swap(0, 2);
            }
            dst[3] = if format == VideoFormat::RGBA {
                src[3]
            } else {
                255
            };
        }
    }
    Ok(Frame::new(width, height, raw))
}

impl WaylandVideoRecorder {
    pub fn new(monitor: ImplMonitor) -> XCapResult<(Self, Receiver<Frame>)> {
        let (sender, receiver) = sync_channel(2);
        let (active_sender, active_receiver) = channel::channel();
        let (ready_tx, ready_rx) = sync_channel(1);

        let (stream_id, remote, session) = pollster::block_on(async {
            let proxy = Screencast::new()
                .await
                .map_err(|e| XCapError::new(e.to_string()))?;

            let session = proxy
                .create_session(Default::default())
                .await
                .map_err(|e| XCapError::new(e.to_string()))?;

            let result = async {
                proxy
                    .select_sources(
                        &session,
                        SelectSourcesOptions::default()
                            .set_sources(Some(SourceType::Monitor.into())),
                    )
                    .await
                    .map_err(|e| XCapError::new(e.to_string()))?;

                let response = proxy
                    .start(&session, None, Default::default())
                    .await
                    .map_err(|e| XCapError::new(e.to_string()))?
                    .response()
                    .map_err(|e| XCapError::new(e.to_string()))?;

                let stream = response
                    .streams()
                    .first()
                    .ok_or_else(|| XCapError::new("no stream found / selected"))?
                    .to_owned();

                let remote = proxy
                    .open_pipe_wire_remote(&session, Default::default())
                    .await
                    .map_err(XCapError::new)?;
                Ok::<_, XCapError>((stream.pipe_wire_node_id(), remote))
            }
            .await;
            match result {
                Ok((stream_id, remote)) => Ok((stream_id, remote, session)),
                Err(err) => {
                    let _ = session.close().await;
                    Err(err)
                }
            }
        })?;

        let is_running = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let worker_running = is_running.clone();
        let worker_alive = alive.clone();

        let handle = thread::spawn(move || {
            let result = (|| -> XCapResult<()> {
                pipewire::init();

                let main_loop = MainLoopRc::new(None)?;
                let context = ContextRc::new(&main_loop, None)?;
                let core = context.connect_fd_rc(remote, None)?;

                let user_data = ListenerUserData {
                    format: Default::default(),
                };

                let stream = StreamRc::new(
                    core.clone(),
                    "XCap",
                    properties::properties! {
                        *MEDIA_TYPE => "Video",
                        *MEDIA_CATEGORY => "Capture",
                        *MEDIA_ROLE => "Screen",
                    },
                )?;

                let frame_loop = main_loop.clone();
                let error_loop = main_loop.clone();
                let control_running = worker_running.clone();
                let _listener = stream
                    .add_local_listener_with_user_data(user_data)
                    .state_changed(move |_, _, _, state| {
                        if let StreamState::Error(err) = state {
                            log::error!("PipeWire stream failed: {err}");
                            error_loop.quit();
                        }
                    })
                    .param_changed(|_, user_data, id, param| {
                        let Some(param) = param else {
                            return;
                        };

                        if id != ParamType::Format.as_raw() {
                            return;
                        }

                        let (media_type, media_subtype) = match format_utils::parse_format(param) {
                            Ok(v) => v,
                            Err(err) => {
                                log::error!("Failed to parse format: {err:?}");
                                return;
                            }
                        };

                        if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                            return;
                        }

                        if let Err(err) = user_data.format.parse(param) {
                            log::error!("Failed to parse format: {err:?}");
                        }
                    })
                    .process(move |stream, user_data| {
                        let state = worker_running.load(Ordering::Acquire);
                        match stream.dequeue_buffer() {
                            None => log::info!("stream.dequeue_buffer() returned None"),
                            Some(mut buffer) => {
                                let datas = buffer.datas_mut();
                                if datas.is_empty() {
                                    return;
                                }
                                if !state || datas[0].as_raw().chunk.is_null() {
                                    return;
                                }
                                let size = user_data.format.size();
                                let chunk = datas[0].chunk();
                                let offset = chunk.offset() as usize;
                                let chunk_size = chunk.size() as usize;
                                let stride = chunk.stride();
                                if let Some(frame_data) = datas[0].data() {
                                    let frame = match decode_frame(
                                        frame_data,
                                        offset,
                                        chunk_size,
                                        stride,
                                        user_data.format.format(),
                                        size.width,
                                        size.height,
                                    ) {
                                        Ok(frame) => frame,
                                        Err(err) => {
                                            log::error!("Invalid PipeWire frame: {err}");
                                            return;
                                        }
                                    };
                                    if worker_running.load(Ordering::Acquire) {
                                        match sender.try_send(frame) {
                                            Ok(()) | Err(TrySendError::Full(_)) => {}
                                            Err(TrySendError::Disconnected(_)) => frame_loop.quit(),
                                        }
                                    }
                                }
                            }
                        }
                    })
                    .register()?;

                let obj = pod::object!(
                    SpaTypes::ObjectParamFormat,
                    ParamType::EnumFormat,
                    pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
                    pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
                    pod::property!(
                        FormatProperties::VideoFormat,
                        Choice,
                        Enum,
                        Id,
                        VideoFormat::RGB,
                        VideoFormat::RGBA,
                        VideoFormat::RGBx,
                        VideoFormat::BGRx,
                        // VideoFormat::YUY2,
                        // VideoFormat::I420,
                    ),
                    pod::property!(
                        FormatProperties::VideoSize,
                        Choice,
                        Range,
                        Rectangle,
                        Rectangle {
                            width: 128,
                            height: 128
                        },
                        Rectangle {
                            width: 1,
                            height: 1
                        },
                        Rectangle {
                            width: 4096,
                            height: 4096
                        }
                    ),
                    pod::property!(
                        FormatProperties::VideoFramerate,
                        Choice,
                        Range,
                        Fraction,
                        Fraction { num: 24, denom: 1 },
                        Fraction { num: 0, denom: 1 },
                        Fraction {
                            num: 1000,
                            denom: 1
                        }
                    ),
                );
                let values =
                    PodSerializer::serialize(Cursor::new(Vec::new()), &pod::Value::Object(obj))
                        .map_err(XCapError::new)?
                        .0
                        .into_inner();

                let mut params =
                    [Pod::from_bytes(&values).ok_or(XCapError::new("Failed to create Pod"))?];

                stream.connect(
                    Direction::Input,
                    Some(stream_id),
                    StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::INACTIVE,
                    &mut params,
                )?;

                // Used to pause/resume the stream
                let control_loop = main_loop.clone();
                let _attached = active_receiver.attach(main_loop.loop_(), {
                    move |command| match command {
                        RecorderCommand::Shutdown => control_loop.quit(),
                        RecorderCommand::Active(active, reply) => {
                            let result = (|| -> XCapResult<()> {
                                if !active {
                                    control_running.store(false, Ordering::Release);
                                }
                                stream.set_active(active)?;
                                if !active {
                                    stream.flush(true)?;
                                }
                                control_running.store(active, Ordering::Release);
                                Ok(())
                            })();
                            let _ = reply.try_send(result);
                        }
                    }
                });

                ready_tx.send(Ok(())).map_err(XCapError::new)?;
                main_loop.run();

                Result::<(), XCapError>::Ok(())
            })();
            worker_alive.store(false, Ordering::Release);
            if let Err(err) = result {
                log::error!("PipeWire recorder failed: {err}");
                let _ = ready_tx.send(Err(err));
            }
            if let Err(err) = pollster::block_on(session.close()) {
                log::error!("Portal session close failed: {err}");
            }
        });

        let recorder = Self {
            monitor,
            shared: Arc::new(RecorderRuntime {
                is_running,
                alive,
                active_sender: Mutex::new(active_sender),
                handle: Some(handle),
            }),
        };
        ready_rx.recv().map_err(XCapError::new)??;
        Ok((recorder, receiver))
    }

    pub fn start(&self) -> XCapResult<()> {
        self.set_active(true)
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.set_active(false)
    }

    fn set_active(&self, active: bool) -> XCapResult<()> {
        let sender = self.shared.active_sender.lock()?;
        if !self.shared.alive.load(Ordering::Acquire) {
            return Err(XCapError::new("PipeWire recorder worker has exited"));
        }
        let (reply, receiver) = sync_channel(1);
        sender
            .send(RecorderCommand::Active(active, reply))
            .map_err(|_| XCapError::new("PipeWire control channel failed"))?;
        receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(XCapError::new)?
    }
}
