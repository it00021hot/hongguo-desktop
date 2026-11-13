mod cancellation;
mod direct;
mod response;
mod source;

use axum::{Router, routing::get};
use source::Source;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
struct Routes {
    authority: String,
    sources: Arc<Mutex<HashMap<String, Source>>>,
}

impl Routes {
    fn source(
        &self,
        id: &str,
        headers: &axum::http::HeaderMap,
    ) -> Result<Source, axum::http::StatusCode> {
        use axum::http::{StatusCode, header};
        if headers.contains_key(header::ORIGIN) {
            return Err(StatusCode::FORBIDDEN);
        }
        if headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            != Some(self.authority.as_str())
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        self.sources
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .get(id)
            .cloned()
            .ok_or(StatusCode::NOT_FOUND)
    }
}

pub(crate) struct MediaServer {
    routes: Routes,
    task: JoinHandle<()>,
}

impl MediaServer {
    pub(crate) async fn start() -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| error.to_string())?;
        let routes = Routes {
            authority: listener
                .local_addr()
                .map_err(|error| error.to_string())?
                .to_string(),
            sources: Arc::new(Mutex::new(HashMap::new())),
        };
        let router = Router::new()
            .route("/media/{id}", get(response::serve))
            .with_state(routes.clone());
        let task = hongguo_service::runtime::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                log::error!("[NSH media] serve failed: {error}");
            }
        });
        Ok(Self { routes, task })
    }

    #[cfg(test)]
    pub(crate) fn file(&self, path: &Path) -> Result<MediaLease, String> {
        self.publish(uuid::Uuid::new_v4().to_string(), Source::file(path)?, None)
    }

    pub(crate) fn playback_file(&self, path: &Path) -> Result<MediaLease, String> {
        self.register(Source::file(path)?)
    }

    pub(crate) fn online(&self, vid: String, definition: u32) -> Result<MediaLease, String> {
        self.register(Source::online(vid, definition)?)
    }

    fn register(&self, source: Source) -> Result<MediaLease, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(
            hongguo_softdec::DirectSession::open(Arc::new(source.clone()))
                .map_err(|error| error.to_string())?,
        );
        let frames = hongguo_nsh_video_api::frames::register_frames(
            &id,
            Arc::new(direct::Frames::new(session.clone())),
        )?;
        self.publish(id, source.audio(session), Some(frames))
    }

    fn publish(
        &self,
        id: String,
        source: Source,
        frames: Option<hongguo_nsh_video_api::frames::FrameRegistration>,
    ) -> Result<MediaLease, String> {
        self.routes
            .sources
            .lock()
            .map_err(|_| "media routes lock poisoned")?
            .insert(id.clone(), source);
        Ok(MediaLease {
            url: format!("http://{}/media/{id}", self.routes.authority),
            id,
            sources: self.routes.sources.clone(),
            frames,
        })
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.task.abort();
        if let Ok(mut sources) = self.routes.sources.lock() {
            for source in sources.values() {
                source.revoke();
            }
            sources.clear();
        }
    }
}

pub(crate) struct MediaLease {
    id: String,
    url: String,
    sources: Arc<Mutex<HashMap<String, Source>>>,
    frames: Option<hongguo_nsh_video_api::frames::FrameRegistration>,
}

impl MediaLease {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn frame_source(&self) -> Option<&str> {
        self.frames.as_ref().map(|frames| frames.url())
    }
}

impl Drop for MediaLease {
    fn drop(&mut self) {
        drop(self.frames.take());
        if let Ok(mut sources) = self.sources.lock()
            && let Some(source) = sources.remove(&self.id)
        {
            source.revoke();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/media.rs"]
mod tests;
