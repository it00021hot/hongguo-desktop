use crate::pages::SessionHandle;
use hongguo_common::{AppError, AppResult, model::LocalHistoryItem};
use hongguo_service::AppState;
use nsh_core::prelude::*;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;

const SAVE_INTERVAL: Duration = Duration::from_secs(5);
const CLOUD_INTERVAL: Duration = Duration::from_secs(30);
const MILLIS_PER_SECOND: f64 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProgressPosition {
    at: f64,
    duration: f64,
}

impl ProgressPosition {
    fn new(at: f64, duration: f64) -> Option<Self> {
        (at.is_finite() && duration.is_finite() && at >= 0.0 && duration > 0.0).then(|| Self {
            at: at.min(duration),
            duration,
        })
    }

    fn history(self, mut history: LocalHistoryItem) -> LocalHistoryItem {
        history.position_ms = (self.at * MILLIS_PER_SECOND) as i64;
        history.duration_ms = (self.duration * MILLIS_PER_SECOND) as i64;
        history.updated_at_ms = hongguo_common::utils::time::now_ms();
        history
    }
}

#[derive(Clone, Copy, Default)]
struct Update {
    position: Option<ProgressPosition>,
    flush: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Recorder(Stored<watch::Sender<Update>>);

impl Recorder {
    pub(super) fn record(self, at: f64, duration: f64) {
        if let Some(position) = ProgressPosition::new(at, duration) {
            self.0
                .with(|sender| sender.send_modify(|update| update.position = Some(position)));
        }
    }

    pub(super) fn flush(self) {
        self.0.with(|sender| {
            sender.send_modify(|update| update.flush = update.flush.wrapping_add(1))
        });
    }

    pub(super) fn complete(self) {
        self.0.with(|sender| {
            sender.send_modify(|update| {
                if let Some(position) = &mut update.position {
                    position.at = position.duration;
                }
                update.flush = update.flush.wrapping_add(1);
            })
        });
    }
}

pub(super) fn observe(
    session: SessionHandle,
    history: LocalHistoryItem,
    visible: Memo<bool>,
) -> Recorder {
    let state = session.with(|session| session.state.clone());
    let revision = Arc::new(AtomicU64::new(0));
    let recorder = start_writer(state, history, revision.clone());
    effect(move || {
        if !visible.get() {
            recorder.flush();
        }
    });
    spawn(async move {
        let mut last_revision = 0;
        loop {
            timer(SAVE_INTERVAL).await;
            let current_revision = revision.load(Ordering::Acquire);
            if current_revision != last_revision {
                session.notify_library();
                last_revision = current_revision;
            }
        }
    });
    recorder
}

fn start_writer(state: AppState, history: LocalHistoryItem, revision: Arc<AtomicU64>) -> Recorder {
    let (sender, receiver) = watch::channel(Update::default());
    // The source owner holds the only sender; its disposal flushes and ends the writer.
    let recorder = Recorder(stored(sender));
    let cloud_receiver = receiver.clone();
    let cloud_state = state.clone();
    let cloud_history = history.clone();
    let cloud_revision = revision.clone();
    // An independent receiver preserves the final report after UI disposal;
    // network latency never holds up the local position writer.
    hongguo_service::runtime::spawn(run_interval_writer(
        cloud_receiver,
        CLOUD_INTERVAL,
        move |position| {
            let state = cloud_state.clone();
            let item = position.history(cloud_history.clone());
            let revision = cloud_revision.clone();
            async move {
                let result = hongguo_service::history_service::record(&state, &item).await;
                if result.is_ok() {
                    revision.fetch_add(1, Ordering::Release);
                }
                if let Err(error) = &result {
                    log::error!(
                        "[NSH playback] history report failed for {}/{}: {error}",
                        item.series_id,
                        item.vid_index
                    );
                }
                result
            }
        },
    ));
    hongguo_service::runtime::spawn(run_writer(receiver, move |position| {
        let state = state.clone();
        let revision = revision.clone();
        let history = position.history(history.clone());
        async move {
            let result = persist_local(state, history, position).await;
            if result.is_ok() {
                revision.fetch_add(1, Ordering::Release);
            }
            result
        }
    }));
    recorder
}

async fn persist_local(
    state: AppState,
    history: LocalHistoryItem,
    position: ProgressPosition,
) -> AppResult<()> {
    let identity = (history.series_id.clone(), history.vid_index);
    let result = hongguo_service::runtime::spawn_blocking(move || {
        hongguo_service::play_service::position::save_store(
            &state.store,
            &history.series_id,
            history.vid_index,
            position.at,
            position.duration,
        )
        .and_then(|()| state.store.upsert_local_history(&history))
    })
    .await
    .map_err(|error| AppError::Internal(format!("NSH progress writer: {error}")))
    .and_then(|result| result);
    if let Err(error) = &result {
        log::error!(
            "[NSH playback] progress save failed for {}/{}: {error}",
            identity.0,
            identity.1
        );
    }
    result
}

async fn run_writer<F, Fut>(receiver: watch::Receiver<Update>, save: F)
where
    F: FnMut(ProgressPosition) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    run_interval_writer(receiver, SAVE_INTERVAL, save).await;
}

async fn run_interval_writer<F, Fut>(
    mut receiver: watch::Receiver<Update>,
    period: Duration,
    mut save: F,
) where
    F: FnMut(ProgressPosition) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let mut interval = tokio::time::interval(period);
    interval.tick().await;
    let mut last_saved = None;
    let mut last_flush = 0;
    loop {
        let (closed, scheduled) = tokio::select! {
            changed = receiver.changed() => (changed.is_err(), false),
            _ = interval.tick() => (false, true),
        };
        let update = *receiver.borrow_and_update();
        let flush = closed || scheduled || update.flush != last_flush;
        last_flush = update.flush;
        if let Some(position) = update.position
            && flush
            && last_saved != Some(position)
            && save(position).await.is_ok()
        {
            last_saved = Some(position);
        }
        if closed {
            break;
        }
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
