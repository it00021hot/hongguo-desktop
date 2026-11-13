use super::{CompatPlay, backend_label};
use crate::transcode_service::cache;
use hongguo_common::{AppError, AppResult};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, LazyLock, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

enum Phase {
    Pending,
    Ready,
    Done,
    Failed(String),
    Cancelled,
}

pub(super) struct Session {
    native: bool,
    cancelled: AtomicBool,
    phase: Mutex<Phase>,
    changed: Condvar,
}

static SLOTS: LazyLock<Mutex<HashMap<PathBuf, Arc<Session>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn slots() -> MutexGuard<'static, HashMap<PathBuf, Arc<Session>>> {
    SLOTS.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub(super) fn begin(path: &Path, native: bool) -> AppResult<Arc<Session>> {
    let mut slots = slots();
    if slots.contains_key(path) {
        return Err(AppError::Busy("这一档正在转码".into()));
    }
    hongguo_protocol::grow::open(path).map_err(AppError::Media)?;
    let session = Arc::new(Session {
        native,
        cancelled: AtomicBool::new(false),
        phase: Mutex::new(Phase::Pending),
        changed: Condvar::new(),
    });
    slots.insert(path.to_owned(), session.clone());
    Ok(session)
}

pub(super) fn clear(path: &Path, session: &Arc<Session>) {
    let mut slots = slots();
    if slots
        .get(path)
        .is_some_and(|current| Arc::ptr_eq(current, session))
    {
        slots.remove(path);
    }
}

pub(super) fn cancel(path: &Path) {
    let slots = slots();
    if let Some(session) = slots.get(path) {
        session.cancel();
        hongguo_protocol::grow::cancel(path);
    }
}

impl Session {
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn ready(&self) {
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if matches!(*phase, Phase::Pending) {
            *phase = Phase::Ready;
            self.changed.notify_all();
        }
    }
    pub fn settle<T>(&self, result: &AppResult<T>) {
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !matches!(*phase, Phase::Pending | Phase::Ready) {
            return;
        }
        *phase = match result {
            Ok(_) => Phase::Done,
            Err(AppError::Cancelled) => Phase::Cancelled,
            Err(error) => Phase::Failed(error.to_string()),
        };
        self.changed.notify_all();
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.settle(&Err::<(), _>(AppError::Cancelled));
    }
}

struct CancelWait(Arc<AtomicBool>);
impl Drop for CancelWait {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(super) async fn attach(path: &Path) -> AppResult<Option<CompatPlay>> {
    let session = {
        let slots = slots();
        let session = slots.get(path).filter(|session| session.native);
        if session.is_some() && hongguo_protocol::grow::snapshot(path).is_none() {
            return Err(AppError::Media("兼容转码会话缺少增长登记".into()));
        }
        session.cloned()
    };
    let Some(session) = session else {
        let path = path.to_owned();
        return crate::runtime::spawn_blocking(move || {
            if !cache::is_complete(&path) {
                return Ok(None);
            }
            play(&path, true, false, 0).map(Some)
        })
        .await
        .map_err(|error| AppError::Media(format!("检查兼容缓存失败: {error}")))?;
    };
    let started = Instant::now();
    let cancellation = CancelWait(Arc::new(AtomicBool::new(false)));
    let cancelled = cancellation.0.clone();
    let target = path.to_owned();
    let progressive = crate::runtime::spawn_blocking(move || session.wait(&target, &cancelled))
        .await
        .map_err(|error| AppError::Media(format!("等待兼容转码首段失败: {error}")))??;
    play(path, false, progressive, started.elapsed().as_millis()).map(Some)
}

fn play(path: &Path, cached: bool, progressive: bool, elapsed_ms: u128) -> AppResult<CompatPlay> {
    let url = hongguo_protocol::local::local_play_url(&path.to_string_lossy())
        .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
    Ok(CompatPlay {
        url,
        cached,
        backend: backend_label(hongguo_media_platform::capability::selected_backend()),
        elapsed_ms,
        progressive,
    })
}

impl Session {
    fn wait(&self, path: &Path, cancelled: &AtomicBool) -> AppResult<bool> {
        const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(100);
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(AppError::Cancelled);
            }
            match &*phase {
                Phase::Ready => return Ok(true),
                Phase::Done => {
                    drop(phase);
                    return if cache::is_complete(path) {
                        Ok(false)
                    } else {
                        Err(AppError::Media("兼容转码完成但产物未封存".into()))
                    };
                }
                Phase::Failed(message) => return Err(AppError::Media(message.clone())),
                Phase::Cancelled => return Err(AppError::Cancelled),
                Phase::Pending => {}
            }
            phase = self
                .changed
                .wait_timeout(phase, CANCEL_CHECK_INTERVAL)
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
