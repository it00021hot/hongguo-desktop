use hongguo_common::{AppError, AppResult};
use hongguo_service::{AppState, AppStateInner};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};
use tokio::task::JoinHandle;

use crate::media::MediaServer;

static MOBILE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The mobile host must supply its private absolute directory before mounting App.
pub fn configure_mobile_data_root(path: &Path) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::InvalidArgs(
            "NSH data root must be absolute".into(),
        ));
    }
    if let Some(root) = MOBILE_ROOT.get() {
        return if root == path {
            Ok(())
        } else {
            Err(AppError::Internal(
                "NSH data root already configured".into(),
            ))
        };
    }
    MOBILE_ROOT
        .set(path.to_owned())
        .map_err(|_| AppError::Internal("NSH data root initialization raced".into()))
}

pub(crate) struct Session {
    pub(crate) state: AppState,
    pub(crate) media: MediaServer,
    pub(crate) personal: crate::personal_sources::PersonalSources,
    registration: JoinHandle<()>,
    follow_notices: OnceLock<JoinHandle<()>>,
}

pub(crate) async fn open() -> AppResult<Arc<Session>> {
    let state = hongguo_service::runtime::spawn_blocking(open_state)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    let media = MediaServer::start().await.map_err(AppError::Internal)?;
    hongguo_service::device::arm_cold_start();
    let registration = hongguo_service::runtime::spawn({
        let state = state.clone();
        async move { hongguo_service::device::ensure_registered(&state).await }
    });
    Ok(Arc::new(Session {
        state,
        media,
        personal: crate::personal_sources::PersonalSources::default(),
        registration,
        follow_notices: OnceLock::new(),
    }))
}

fn open_state() -> AppResult<AppState> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        let root = MOBILE_ROOT.get().ok_or_else(|| {
            AppError::Internal("NSH mobile host did not configure its data directory".into())
        })?;
        hongguo_store::paths::set_absolute_data_root(root);
    } else {
        hongguo_store::paths::set_shell_data_root("hongguo-nsh");
    }
    let store = hongguo_service::device::open_application_store(&hongguo_store::paths::db_file())?;
    let settings = match store.settings()? {
        Some(settings) => hongguo_service::settings_service::normalize(settings)?,
        None => {
            let mut settings = hongguo_common::model::Settings::default();
            if cfg!(any(target_os = "android", target_os = "ios")) {
                settings.download_dir = hongguo_store::paths::data_dir()
                    .join("downloads")
                    .to_string_lossy()
                    .into_owned();
            }
            settings
        }
    };
    let tasks = store.tasks()?;
    let (device, _) = hongguo_service::device::adopt_sync(&store);
    let state = Arc::new(AppStateInner::with_device(store, device));
    hongguo_service::settings_service::apply_cache_root(&settings);
    state.replace_queue(
        hongguo_service::download_service::queue::DownloadQueue::restore(
            tasks,
            settings.max_concurrency,
        ),
    );
    state.replace_settings(settings);
    Ok(state)
}

impl Session {
    #[cfg(test)]
    pub(crate) async fn for_test(
        state: AppState,
        history: Vec<hongguo_api::history::WatchHistoryItem>,
    ) -> AppResult<Arc<Self>> {
        Ok(Arc::new(Self {
            personal: crate::personal_sources::PersonalSources::with_test_history(&state, history),
            state,
            media: MediaServer::start().await.map_err(AppError::Internal)?,
            registration: hongguo_service::runtime::spawn(async {}),
            follow_notices: OnceLock::new(),
        }))
    }

    pub(crate) fn start_notice_checks(&self) {
        self.follow_notices
            .get_or_init(|| hongguo_service::follow_service::start_notice_task(self.state.clone()));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.registration.abort();
        if let Some(task) = self.follow_notices.get() {
            task.abort();
        }
        self.state.scheduler().pause_all();
        self.state.queue().pause_all();
    }
}
