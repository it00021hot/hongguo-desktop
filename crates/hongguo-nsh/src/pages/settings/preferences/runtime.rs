use crate::{
    bridge,
    events::SessionHandle,
    i18n::{format_bytes, tr},
    navigation::{Navigation, Route},
};
use hongguo_common::{AppError, AppResult};
use hongguo_media_platform::capability::probe::{self, DecodeCapability};
use hongguo_service::{
    AppState, runtime,
    storage_service::{cache, usage},
};
use nsh_core::prelude::*;

#[derive(Clone)]
pub(super) struct Usage {
    pub(super) downloads: usage::StorageUsage,
    pub(super) cache: cache::CacheUsage,
}

#[derive(Clone)]
enum Backend {
    Production(AppState),
    Preview,
}

#[derive(Clone, Copy)]
pub(super) struct Runtime {
    pub(super) usage: Resource<Usage, bridge::Failure>,
    pub(super) capability: Memo<Option<DecodeCapability>>,
    pub(super) probing: Memo<bool>,
    pub(super) clearing: Signal<bool>,
    backend: Stored<Backend>,
    probe_pending: Signal<bool>,
    probe_value: Signal<Option<DecodeCapability>>,
    revision: Signal<u64>,
    message: Signal<SharedString>,
}

impl Runtime {
    pub(super) fn production(
        session: SessionHandle,
        nav: Navigation,
        message: Signal<SharedString>,
    ) -> Self {
        Self::new(
            Backend::Production(session.with(|session| session.state.clone())),
            nav,
            message,
        )
    }
    pub(super) fn preview(nav: Navigation, message: Signal<SharedString>) -> Self {
        Self::new(Backend::Preview, nav, message)
    }

    fn new(backend: Backend, nav: Navigation, message: Signal<SharedString>) -> Self {
        let backend = stored(backend);
        let revision = signal(0);
        let active = move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::Settings))
        };
        let usage = resource(
            move || active().then(|| revision.get()),
            move |_, _| {
                let backend = backend.with(|backend| backend.clone());
                async move {
                    match backend {
                        Backend::Production(state) => {
                            bridge::request(async move {
                                blocking(move || {
                                    Ok(Usage {
                                        downloads: usage::usage(&state),
                                        cache: cache::usage(),
                                    })
                                })
                                .await
                            })
                            .await
                        }
                        Backend::Preview => Ok(Usage {
                            downloads: usage::StorageUsage { bytes: 0, files: 0 },
                            cache: cache::CacheUsage::default(),
                        }),
                    }
                }
            },
        );
        let detected = resource(
            move || active().then_some(()),
            move |_, _| {
                let preview = backend.with(|backend| matches!(backend, Backend::Preview));
                async move {
                    if preview {
                        Ok(DecodeCapability {
                            hevc_hw_encoder: true,
                            platform_encoder: true,
                            platform_hw_encoder: true,
                            ..Default::default()
                        })
                    } else {
                        bridge::request(async { blocking(|| Ok(probe::detect())).await }).await
                    }
                }
            },
        );
        let probe_value = signal(None);
        let probe_pending = signal(false);
        Self {
            usage,
            capability: computed(move || probe_value.get().or_else(|| detected.get())),
            probing: computed(move || probe_pending.get() || detected.loading()),
            clearing: signal(false),
            backend,
            probe_pending,
            probe_value,
            revision,
            message,
        }
    }

    pub(super) fn redetect(self) {
        if self.probing.get_untracked() {
            return;
        }
        self.probe_pending.set(true);
        let preview = self
            .backend
            .with(|backend| matches!(backend, Backend::Preview));
        spawn(async move {
            let outcome = if preview {
                Ok(DecodeCapability {
                    hevc_hw_encoder: true,
                    platform_encoder: true,
                    platform_hw_encoder: true,
                    ..Default::default()
                })
            } else {
                bridge::request(async { blocking(|| Ok(probe::redetect())).await }).await
            };
            match outcome {
                Ok(capability) => {
                    self.probe_value.set(Some(capability));
                    self.message.set(
                        tr("settings.redetectDone")
                            .replace("{backend}", &backend_label(Some(capability)))
                            .into(),
                    );
                }
                Err(error) => self.message.set(error.text()),
            }
            self.probe_pending.set(false);
        });
    }

    pub(super) fn clear(self) {
        if self.clearing.get_untracked() {
            return;
        }
        self.clearing.set(true);
        let preview = self
            .backend
            .with(|backend| matches!(backend, Backend::Preview));
        spawn(async move {
            let outcome = if preview {
                Ok(cache::CacheClearResult { items: 0, bytes: 0 })
            } else {
                bridge::request(async { blocking(|| Ok(cache::clear())).await }).await
            };
            match outcome {
                Ok(result) => {
                    self.revision.update(|value| *value += 1);
                    self.message.set(
                        tr("settings.cacheCleared")
                            .replace("{count}", &result.items.to_string())
                            .replace("{size}", &format_bytes(result.bytes))
                            .into(),
                    );
                }
                Err(error) => self.message.set(error.text()),
            }
            self.clearing.set(false);
        });
    }
}

async fn blocking<T: Send + 'static>(
    run: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    runtime::spawn_blocking(run)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

pub(super) fn backend_label(capability: Option<DecodeCapability>) -> SharedString {
    tr(match capability {
        None => "common.loading",
        Some(cap) if cap.platform_hw_encoder || cap.hevc_hw_encoder => "settings.backendFfmpegHw",
        Some(cap) if cap.platform_encoder || cap.has_ffmpeg => "settings.backendFfmpegSw",
        Some(_) => "settings.backendUnavailable",
    })
}
