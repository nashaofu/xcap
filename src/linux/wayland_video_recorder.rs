use std::{
    collections::HashMap,
    fmt,
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use pipewire::{
    channel,
    context::Context,
    keys::{MEDIA_CATEGORY, MEDIA_ROLE, MEDIA_TYPE},
    main_loop::MainLoop,
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
    stream::{Stream, StreamFlags},
};
use zbus::{
    blocking::Proxy,
    zvariant::{DeserializeDict, OwnedFd, OwnedObjectPath, Type, Value},
};

use crate::{XCapError, XCapResult, video_recorder::Frame};

use super::{
    impl_monitor::ImplMonitor,
    utils::{get_zbus_connection, get_zbus_portal_request, wait_zbus_response},
};

#[allow(dead_code)]
#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
pub struct ScreenCastCreateSessionResponse {
    session_handle: String,
}

#[allow(dead_code)]
#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
pub struct ScreenCastStartStream {
    pub id: Option<String>,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub source_type: Option<u32>,
    pub mapping_id: Option<String>,
}

#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
pub struct ScreenCastStartResponse {
    pub streams: Option<Vec<(u32, ScreenCastStartStream)>>,
    #[allow(dead_code)]
    pub restore_token: Option<String>,
}

/// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
pub struct ScreenCast<'a> {
    proxy: Proxy<'a>,
}

impl ScreenCast<'_> {
    pub fn new() -> XCapResult<Self> {
        let conn = get_zbus_connection()?;
        let proxy = Proxy::new(
            conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.ScreenCast",
        )?;

        Ok(ScreenCast { proxy })
    }

    pub fn create_session(&self) -> XCapResult<OwnedObjectPath> {
        let conn = get_zbus_connection()?;

        let mut options = HashMap::new();

        let handle_token = rand::random::<u32>().to_string();
        let portal_request = get_zbus_portal_request(conn, &handle_token)?;

        options.insert("handle_token", Value::from(&handle_token));

        let session_handle_token = rand::random::<u32>().to_string();
        options.insert("session_handle_token", Value::from(&session_handle_token));

        self.proxy.call_method("CreateSession", &(options))?;

        let response: ScreenCastCreateSessionResponse = wait_zbus_response(&portal_request)?;

        let unique_name = conn
            .unique_name()
            .ok_or(XCapError::new("Failed to get unique name"))?;
        let unique_identifier = unique_name.trim_start_matches(':').replace('.', "_");

        let session = OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/session/{unique_identifier}/{session_handle_token}"
        ))?;

        if session.as_str() != response.session_handle {
            return Err(XCapError::new("Session handle mismatch"));
        }

        Ok(session)
    }

    pub fn select_sources(&self, session: &OwnedObjectPath) -> XCapResult<()> {
        let conn = get_zbus_connection()?;

        let mut options = HashMap::new();

        let handle_token = rand::random::<u32>().to_string();
        let portal_request = get_zbus_portal_request(conn, &handle_token)?;

        options.insert("handle_token", Value::from(handle_token));
        options.insert("types", Value::from(1_u32));
        options.insert("multiple", Value::from(false));

        self.proxy
            .call_method("SelectSources", &(session, options))?;

        portal_request.receive_signal("Response")?;

        Ok(())
    }

    pub fn start(&self, session: &OwnedObjectPath) -> XCapResult<ScreenCastStartResponse> {
        let conn = get_zbus_connection()?;

        let mut options = HashMap::new();

        let handle_token = rand::random::<u32>().to_string();
        let portal_request = get_zbus_portal_request(conn, &handle_token)?;

        options.insert("handle_token", Value::from(&handle_token));

        self.proxy.call_method("Start", &(session, "", options))?;

        wait_zbus_response(&portal_request)
    }

    #[allow(dead_code)]
    pub fn open_pipe_wire_remote(&self, session: &OwnedObjectPath) -> XCapResult<OwnedFd> {
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let fd: OwnedFd = self.proxy.call("OpenPipeWireRemote", &(session, options))?;

        Ok(fd)
    }
}

#[derive(Clone)]
pub struct WaylandVideoRecorder {
    #[allow(dead_code)]
    monitor: ImplMonitor,
    sender: Sender<Frame>,
    is_running: Arc<AtomicBool>,
    active_sender: channel::Sender<bool>,
}

impl fmt::Debug for WaylandVideoRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaylandVideoRecorder")
            .field("monitor", &self.monitor)
            .field("sender", &self.sender)
            .field("is_running", &self.is_running)
            // Sender is not Debug
            // .field("control_tx", &self.control_tx)
            .finish()
    }
}

#[derive(Clone)]
struct ListenerUserData {
    pub format: VideoInfoRaw,
}

impl WaylandVideoRecorder {
    pub fn new(monitor: ImplMonitor) -> XCapResult<(Self, Receiver<Frame>)> {
        let (sender, receiver) = mpsc::channel();
        let (active_sender, active_receiver) = channel::channel();

        let screen_cast = ScreenCast::new()?;
        let session = screen_cast.create_session()?;
        screen_cast.select_sources(&session)?;
        let response = screen_cast.start(&session)?;

        // 获取流节点ID
        let stream_id = response
            .streams
            .ok_or(XCapError::new("Stream ID not found"))?
            .first()
            .ok_or(XCapError::new("Stream ID not found"))?
            .0;

        let recorder = Self {
            monitor,
            sender,
            is_running: Arc::new(AtomicBool::new(false)),
            active_sender,
        };

        recorder.pipewire_capturer(stream_id, active_receiver)?;

        Ok((recorder, receiver))
    }

    pub fn pipewire_capturer(
        &self,
        stream_id: u32,
        active_receiver: channel::Receiver<bool>,
    ) -> XCapResult<()> {
        let sender = self.sender.clone();
        let is_running = self.is_running.clone();

        thread::spawn(move || {
            pipewire::init();

            let main_loop = MainLoop::new(None)?;
            let context = Context::new(&main_loop)?;
            let core = context.connect(None)?;

            let user_data = ListenerUserData {
                format: Default::default(),
            };

            let stream = Stream::new(
                &core,
                "XCap",
                properties::properties! {
                    *MEDIA_TYPE => "Video",
                    *MEDIA_CATEGORY => "Capture",
                    *MEDIA_ROLE => "Screen",
                },
            )?;

            let _listener = stream
                .add_local_listener_with_user_data(user_data)
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
                    let state = is_running.load(Ordering::Relaxed);
                    match stream.dequeue_buffer() {
                        None => log::info!("stream.dequeue_buffer() returned None"),
                        Some(mut buffer) => {
                            let datas = buffer.datas_mut();
                            if datas.is_empty() {
                                return;
                            }
                            let size = user_data.format.size();
                            if let Some(frame_data) = datas[0].data() {
                                let buffer = match user_data.format.format() {
                                    VideoFormat::RGB => {
                                        let mut buf =
                                            vec![0; (size.width * size.height * 4) as usize];
                                        for (src, dst) in
                                            frame_data.chunks_exact(3).zip(buf.chunks_exact_mut(4))
                                        {
                                            dst[0] = src[0];
                                            dst[1] = src[1];
                                            dst[2] = src[2];
                                            dst[3] = 255;
                                        }

                                        buf
                                    }
                                    VideoFormat::RGBA => frame_data.to_vec(),
                                    VideoFormat::RGBx => frame_data.to_vec(),
                                    VideoFormat::BGRx => {
                                        let mut buf = frame_data.to_vec();
                                        for src in buf.chunks_exact_mut(4) {
                                            src.swap(0, 2);
                                        }

                                        buf
                                    }
                                    _ => {
                                        log::error!(
                                            "Unsupported format: {:?}",
                                            user_data.format.format()
                                        );
                                        return;
                                    }
                                };

                                if state {
                                    let _ =
                                        sender.send(Frame::new(size.width, size.height, buffer));
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
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut params,
            )?;

            // Used to pause/resume the stream
            let _attached = active_receiver.attach(main_loop.loop_(), {
                move |active| {
                    if let Err(e) = stream.set_active(active) {
                        log::error!("Failed to set stream active={active}: {e:?}");
                    }
                    if !active {
                        if let Err(e) = stream.flush(true) {
                            log::error!("Failed to flush: {e:?}");
                        }
                    }
                }
            });

            main_loop.run();

            Result::<(), XCapError>::Ok(())
        });

        Ok(())
    }

    pub fn start(&self) -> XCapResult<()> {
        self.is_running.store(true, Ordering::Relaxed);
        let _ = self.active_sender.send(true);
        Ok(())
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.is_running.store(false, Ordering::Relaxed);
        let _ = self.active_sender.send(false);
        Ok(())
    }
}
