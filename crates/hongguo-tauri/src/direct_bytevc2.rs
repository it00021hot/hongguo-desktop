use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hongguo_bytevc2::{DecodedFrame, IndexedBytevc2};
use hongguo_common::{AppError, AppResult};
use hongguo_stream::range::{RangeSpec, parse_range};
use tauri::http::{Method, Request, Response};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};
#[cfg(target_os = "windows")]
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, ICoreWebView2_17, ICoreWebView2Environment12,
};
#[cfg(target_os = "windows")]
use windows_core::{HSTRING, Interface, PWSTR};

use crate::direct_bytevc2_source::Source;

pub const PATH_PREFIX: &str = "/bvc2/";
const FRAME_BATCH: usize = 8;
const FRAME_QUEUE: usize = 16;
const AUDIO_WINDOW: u64 = 256 * 1024;
const MAX_INDEX_READ: u64 = 16 * 1024 * 1024;
const FRAME_WAIT: Duration = Duration::from_secs(35);

type FrameReceiver = Arc<Mutex<Receiver<AppResult<DecodedFrame>>>>;
type SourceIndex = (Source, Arc<IndexedBytevc2>);

#[derive(Default)]
pub struct DirectPlayback {
    sessions: Mutex<HashMap<String, Session>>,
    indexes: Mutex<HashMap<String, SourceIndex>>,
}

struct Session {
    id: u64,
    cancelled: Arc<AtomicBool>,
    frames: Option<FrameReceiver>,
}

impl DirectPlayback {
    pub fn dispatch(
        self: &Arc<Self>,
        context: UriSchemeContext<'_, Wry>,
        request: Request<Vec<u8>>,
        responder: UriSchemeResponder,
    ) {
        let app = context.app_handle();
        let label = context.webview_label().to_owned();
        let Some(window) = app.get_webview_window(&label) else {
            responder.respond(response(410, "", "text/plain", Vec::new()));
            return;
        };
        let origin = request
            .headers()
            .get("origin")
            .and_then(|value| value.to_str().ok());
        let allowed_origin = match origin {
            Some(origin) if app_origin(app.config(), origin) => Some(origin),
            None | Some("null") if !cfg!(target_os = "windows") => window
                .url()
                .ok()
                .filter(|url| app_origin(app.config(), &page_origin(url)))
                .map(|_| origin.unwrap_or("null")),
            _ => None,
        };
        let Some(origin) = allowed_origin.map(str::to_owned) else {
            responder.respond(response(403, "", "text/plain", Vec::new()));
            return;
        };
        let playback = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let reply = playback.serve(&label, &origin, &request);
            match reply {
                Ok(Reply::Http(result)) => responder.respond(result),
                #[cfg(target_os = "windows")]
                Ok(Reply::Frames { id, bytes }) if !bytes.is_empty() => {
                    let dispatch = window.with_webview(move |webview| {
                        let result = match post_frames(webview, &origin, id, &bytes) {
                            Ok(()) => response(200, &origin, "text/plain", Vec::new()),
                            Err(error) => fail(&origin, error),
                        };
                        responder.respond(result);
                    });
                    if let Err(error) = dispatch {
                        log::debug!("[ByteVC2/Direct] 窗口已关闭: {error}");
                    }
                }
                #[cfg(not(target_os = "windows"))]
                Ok(Reply::Frames { bytes }) if !bytes.is_empty() => {
                    responder.respond(response(200, &origin, "application/octet-stream", bytes));
                }
                Ok(Reply::Frames { .. }) => {
                    responder.respond(response(204, &origin, "text/plain", Vec::new()));
                }
                Err(AppError::Cancelled) => {
                    responder.respond(response(410, &origin, "text/plain", Vec::new()));
                }
                Err(error) => responder.respond(fail(&origin, error)),
            }
        });
    }

    fn serve(&self, label: &str, origin: &str, request: &Request<Vec<u8>>) -> AppResult<Reply> {
        let path = request
            .uri()
            .path()
            .strip_prefix(PATH_PREFIX)
            .unwrap_or_default();
        let parts: Vec<_> = path.split('/').collect();
        match (request.method(), parts.as_slice()) {
            (&Method::GET, ["audio", definition, vid]) => {
                let source = Source::parse(definition, vid)?;
                let range = request
                    .headers()
                    .get("range")
                    .and_then(|value| value.to_str().ok());
                self.audio(label, origin, &source, range).map(Reply::Http)
            }
            (&Method::POST, ["start", id, definition, vid, target]) => {
                let id = session_id(id)?;
                let source = Source::parse(definition, vid)?;
                let target = f64::from(number(target)?) / 1000.0;
                let receiver = self.start(label, id, source, target)?;
                Ok(Reply::Frames {
                    #[cfg(target_os = "windows")]
                    id,
                    bytes: frames(&receiver, 1)?,
                })
            }
            (&Method::POST, ["next", id]) => {
                let id = session_id(id)?;
                let receiver = self
                    .sessions
                    .lock()
                    .expect("sessions mutex")
                    .get(label)
                    .filter(|session| session.id == id)
                    .and_then(|session| session.frames.as_ref())
                    .map(Arc::clone)
                    .ok_or(AppError::Cancelled)?;
                Ok(Reply::Frames {
                    #[cfg(target_os = "windows")]
                    id,
                    bytes: frames(&receiver, FRAME_BATCH)?,
                })
            }
            (&Method::POST, ["stop", id]) => {
                let id = session_id(id)?;
                let mut sessions = self.sessions.lock().expect("sessions mutex");
                let session = sessions
                    .entry(label.to_owned())
                    .or_insert_with(|| Session::stopped(id));
                if id >= session.id {
                    session.cancel();
                    *session = Session::stopped(id);
                }
                Ok(Reply::Http(response(204, origin, "text/plain", Vec::new())))
            }
            _ => Err(AppError::InvalidArgs("无效的 ByteVC2 播放请求".into())),
        }
    }

    fn index(
        &self,
        label: &str,
        source: &Source,
        cancelled: &AtomicBool,
    ) -> AppResult<Arc<IndexedBytevc2>> {
        if let Some((cached, index)) = self.indexes.lock().expect("indexes mutex").get(label)
            && cached == source
        {
            return Ok(Arc::clone(index));
        }
        let mut reader = source.reader()?;
        let index = Arc::new(IndexedBytevc2::open(
            source.length(cancelled)?,
            |offset, length, bytes| {
                if length > MAX_INDEX_READ {
                    return Err(AppError::Media("ByteVC2 索引过大".into()));
                }
                reader.read(offset, length, cancelled, bytes)
            },
        )?);
        self.indexes
            .lock()
            .expect("indexes mutex")
            .insert(label.to_owned(), (source.clone(), Arc::clone(&index)));
        Ok(index)
    }

    fn start(&self, label: &str, id: u64, source: Source, target: f64) -> AppResult<FrameReceiver> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = sync_channel(FRAME_QUEUE);
        let receiver = Arc::new(Mutex::new(receiver));
        {
            let mut sessions = self.sessions.lock().expect("sessions mutex");
            if let Some(previous) = sessions.get(label) {
                if previous.id >= id {
                    return Err(AppError::Cancelled);
                }
                previous.cancel();
            }
            sessions.insert(
                label.to_owned(),
                Session {
                    id,
                    cancelled: Arc::clone(&cancelled),
                    frames: Some(Arc::clone(&receiver)),
                },
            );
        }
        let started = Instant::now();
        let index = self.index(label, &source, &cancelled)?;
        std::thread::Builder::new().name("bytevc2-direct".into()).spawn(move || {
            let mut first = true;
            let result = source.reader().and_then(|mut reader| index.decode_from(target, |offset, length, bytes| {
                reader.read(offset, length, &cancelled, bytes)
            }, |frame| {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(AppError::Cancelled);
                }
                if first {
                    first = false;
                    let pts = frame.pts.unwrap_or_default();
                    log::info!("[ByteVC2/Direct] session={id} target={target:.3}s pts={pts:.3}s decode_ms={}",
                        started.elapsed().as_millis());
                }
                sender.send(Ok(frame)).map_err(|_| AppError::Cancelled)
            }));
            if let Err(error) = result && !matches!(error, AppError::Cancelled) {
                let _ = sender.send(Err(error));
            }
        }).map_err(|error| AppError::Io(error.to_string()))?;
        Ok(receiver)
    }

    fn audio(
        &self,
        label: &str,
        origin: &str,
        source: &Source,
        range: Option<&str>,
    ) -> AppResult<Response<Vec<u8>>> {
        let cancelled = AtomicBool::new(false);
        let total = source.length(&cancelled)?;
        let (status, start, end) = match parse_range(range, total) {
            RangeSpec::Full => (200, 0, total - 1),
            RangeSpec::Closed { start, end } => {
                (206, start, end.min(start.saturating_add(AUDIO_WINDOW - 1)))
            }
            RangeSpec::Open { start } => (
                206,
                start,
                start.saturating_add(AUDIO_WINDOW - 1).min(total - 1),
            ),
            RangeSpec::Unsatisfiable => {
                return Ok(builder(416, origin, "audio/mp4")
                    .header("Content-Range", format!("bytes */{total}"))
                    .body(Vec::new())
                    .expect("range error response"));
            }
        };
        let index = self.index(label, source, &cancelled)?;
        let mut bytes = Vec::new();
        source
            .reader()?
            .read(start, end - start + 1, &cancelled, &mut bytes)?;
        index.mask_video_tracks(start, &mut bytes);
        let mut response = builder(status, origin, "audio/mp4")
            .header("Accept-Ranges", "bytes")
            .header("Content-Length", bytes.len());
        if status == 206 {
            response = response.header("Content-Range", format!("bytes {start}-{end}/{total}"));
        }
        Ok(response.body(bytes).expect("audio response"))
    }
}

impl Session {
    fn stopped(id: u64) -> Self {
        Self {
            id,
            cancelled: Arc::new(AtomicBool::new(true)),
            frames: None,
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

enum Reply {
    Http(Response<Vec<u8>>),
    Frames {
        #[cfg(target_os = "windows")]
        id: u64,
        bytes: Vec<u8>,
    },
}

fn frames(receiver: &Mutex<Receiver<AppResult<DecodedFrame>>>, count: usize) -> AppResult<Vec<u8>> {
    let receiver = receiver.lock().expect("frames mutex");
    let mut bytes = Vec::new();
    for _ in 0..count {
        let frame = match receiver.recv_timeout(FRAME_WAIT) {
            Ok(frame) => frame?,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                return Err(AppError::Network("等待 ByteVC2 解帧超时".into()));
            }
        };
        bytes.extend_from_slice(&(frame.width as u32).to_le_bytes());
        bytes.extend_from_slice(&(frame.height as u32).to_le_bytes());
        bytes.extend_from_slice(&frame.pts.unwrap_or_default().to_le_bytes());
        bytes.extend_from_slice(&frame.y);
        bytes.extend_from_slice(&frame.u);
        bytes.extend_from_slice(&frame.v);
    }
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn post_frames(
    webview: tauri::webview::PlatformWebview,
    origin: &str,
    id: u64,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let core = webview.controller().CoreWebView2()?;
        let mut page = PWSTR::null();
        core.Source(&mut page)?;
        let page = tauri::Url::parse(&webview2_com::take_pwstr(page))?;
        if page.origin().ascii_serialization() != origin {
            return Err("ByteVC2 播放页面已离开".into());
        }
        let core: ICoreWebView2_17 = core.cast()?;
        let environment: ICoreWebView2Environment12 = webview.environment().cast()?;
        let shared = environment.CreateSharedBuffer(bytes.len() as u64)?;
        let mut buffer = std::ptr::null_mut();
        shared.Buffer(&mut buffer)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
        let metadata = HSTRING::from(format!("{{\"bytevc2Session\":{id}}}"));
        let posted = core.PostSharedBufferToScript(
            &shared,
            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
            &metadata,
        );
        let closed = shared.Close();
        Ok(posted.and(closed)?)
    }
}

fn app_origin(config: &tauri::Config, origin: &str) -> bool {
    origin == "http://tauri.localhost"
        || origin == "https://tauri.localhost"
        || origin == "tauri://localhost"
        || (cfg!(debug_assertions)
            && config
                .build
                .dev_url
                .as_ref()
                .is_some_and(|url| url.origin().ascii_serialization() == origin))
}

fn page_origin(url: &tauri::Url) -> String {
    if url.scheme() == "tauri" && url.host_str() == Some("localhost") {
        "tauri://localhost".into()
    } else {
        url.origin().ascii_serialization()
    }
}

fn number(value: &str) -> AppResult<u32> {
    value
        .parse()
        .map_err(|_| AppError::InvalidArgs("无效的 ByteVC2 请求参数".into()))
}

fn session_id(value: &str) -> AppResult<u64> {
    value
        .parse()
        .map_err(|_| AppError::InvalidArgs("无效的 ByteVC2 播放代次".into()))
}

fn builder(status: u16, origin: &str, content_type: &str) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", origin)
        .header("Cache-Control", "no-store")
        .header("Content-Type", content_type)
}

fn response(status: u16, origin: &str, content_type: &str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    builder(status, origin, content_type)
        .body(bytes)
        .expect("ByteVC2 response")
}

fn fail(origin: &str, error: impl std::fmt::Display) -> Response<Vec<u8>> {
    log::warn!("[ByteVC2/Direct] {error}");
    response(
        500,
        origin,
        "text/plain; charset=utf-8",
        error.to_string().into_bytes(),
    )
}
