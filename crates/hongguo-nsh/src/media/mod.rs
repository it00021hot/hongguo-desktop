mod cancellation;
mod hls;
mod hls_source;
#[cfg(any(target_os = "android", test))]
mod native_read;
mod platform;
mod readiness;
mod response;
mod source;

pub(crate) use platform::paused_seek_needs_progress_drive;
pub(crate) use readiness::MediaReadiness;

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
            .route("/media/{id}/playlist.m3u8", get(hls::playlist))
            .route("/media/{id}/init.mp4", get(hls::initialization))
            .route("/media/{id}/segment/{index}", get(hls::segment))
            .with_state(routes.clone());
        let task = hongguo_service::runtime::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                log::error!("[NSH media] serve failed: {error}");
            }
        });
        Ok(Self { routes, task })
    }

    pub(crate) fn file(&self, path: &Path) -> Result<MediaLease, String> {
        self.register(Source::file(path)?)
    }

    pub(crate) async fn playback_file(&self, path: &Path) -> Result<MediaLease, String> {
        platform::wait_for_playable(path).await?;
        #[cfg(target_os = "android")]
        {
            let source = Source::file(path)?;
            if source.is_growing() {
                return self.register_native(source);
            }
        }
        self.file(path)
    }

    #[cfg(any(target_os = "android", test))]
    fn register_native(&self, source: Source) -> Result<MediaLease, String> {
        use hongguo_nsh_video_api::read_source::register_source;
        let mut lease = self.register(source.clone())?;
        let registration = register_source(&lease.id, Arc::new(native_read::NativeRead(source)))
            .map_err(|error| error.to_string())?;
        lease.url = registration.url().to_owned();
        lease.native_read = Some(registration);
        Ok(lease)
    }

    pub(crate) fn online(&self, vid: String, definition: u32) -> Result<MediaLease, String> {
        self.register(Source::online(vid, definition)?)
    }

    fn register(&self, source: Source) -> Result<MediaLease, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let suffix = if source.is_growing() {
            "/playlist.m3u8"
        } else {
            ""
        };
        self.routes
            .sources
            .lock()
            .map_err(|_| "media routes lock poisoned")?
            .insert(id.clone(), source);
        Ok(MediaLease {
            url: format!("http://{}/media/{id}{suffix}", self.routes.authority),
            id,
            sources: self.routes.sources.clone(),
            #[cfg(any(target_os = "android", test))]
            native_read: None,
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
    #[cfg(any(target_os = "android", test))]
    native_read: Option<hongguo_nsh_video_api::read_source::ReadRegistration>,
}

impl MediaLease {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn readiness(&self) -> Result<Option<MediaReadiness>, String> {
        let source = self
            .sources
            .lock()
            .map_err(|_| "media routes lock poisoned".to_owned())?
            .get(&self.id)
            .cloned()
            .ok_or_else(|| "media lease has been revoked".to_owned())?;
        Ok(source.is_growing().then(|| MediaReadiness::new(source)))
    }

    pub(crate) fn is_complete(&self) -> Result<bool, String> {
        self.sources
            .lock()
            .map_err(|_| "media routes lock poisoned".to_owned())?
            .get(&self.id)
            .map(Source::is_complete)
            .transpose()?
            .ok_or_else(|| "media lease has been revoked".to_owned())
    }

    pub(crate) fn available_duration(&self) -> Result<Option<f64>, String> {
        self.sources
            .lock()
            .map_err(|_| "media routes lock poisoned".to_owned())?
            .get(&self.id)
            .map(Source::available_duration)
            .transpose()?
            .ok_or_else(|| "media lease has been revoked".to_owned())
    }
}

impl Drop for MediaLease {
    fn drop(&mut self) {
        #[cfg(any(target_os = "android", test))]
        drop(self.native_read.take());
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
