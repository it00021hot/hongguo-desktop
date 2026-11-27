use crate::features::SessionHandle;
use hongguo_common::{AppError, AppResult, model::LocalHistoryItem};
use hongguo_service::AppState;
use nsh_core::prelude::*;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, watch};

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

/// Synchronous handoff between mounted players, independent of the DB/cloud
/// writer cadence. Scope by login and exact episode, never by series alone.
#[derive(Clone, Copy)]
pub(crate) struct ProgressMemory(Stored<HashMap<EpisodeKey, Arc<EpisodeProgress>>>);

type EpisodeKey = (Option<i64>, String, u32);

impl ProgressMemory {
    pub(crate) fn new() -> Self {
        Self(stored(HashMap::new()))
    }

    fn episode(self, key: EpisodeKey) -> Arc<EpisodeProgress> {
        self.0
            .with(|episodes| episodes.entry(key).or_default().clone())
    }
}

#[derive(Default)]
struct EpisodeProgress {
    latest: Mutex<Option<Checkpoint>>,
    local_write: AsyncMutex<()>,
    cloud_write: AsyncMutex<()>,
}

#[derive(Clone, Copy, PartialEq)]
struct Checkpoint {
    owner: uuid::Uuid,
    position: ProgressPosition,
    completed: bool,
}

impl Checkpoint {
    fn handoff_to(self, owner: uuid::Uuid, seen: Option<Self>) -> Option<f64> {
        (self.owner != owner && seen != Some(self)).then_some(if self.completed {
            0.0
        } else {
            self.position.at
        })
    }
}

impl EpisodeProgress {
    fn latest(&self) -> Option<Checkpoint> {
        *self
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn record(&self, checkpoint: Checkpoint) {
        *self
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(checkpoint);
    }

    fn owns_latest(&self, owner: uuid::Uuid) -> bool {
        self.latest().is_some_and(|latest| latest.owner == owner)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Recorder {
    sender: Stored<watch::Sender<Update>>,
    episode: Stored<Arc<EpisodeProgress>>,
    owner: uuid::Uuid,
    seen: Stored<Option<Checkpoint>>,
}

impl Recorder {
    pub(super) fn record(self, at: f64, duration: f64) {
        if let Some(position) = ProgressPosition::new(at, duration) {
            self.episode.with(|episode| {
                let latest = episode.latest();
                // Ignore a queued native callback until this mount has adopted
                // a newer checkpoint from the other player.
                if latest.is_some_and(|latest| {
                    latest.owner != self.owner && self.seen.get() != Some(latest)
                }) {
                    return;
                }
                episode.record(Checkpoint {
                    owner: self.owner,
                    position,
                    completed: false,
                });
                self.sender.try_with(|sender| {
                    sender.send_modify(|update| update.position = Some(position))
                });
            });
        }
    }

    pub(super) fn take_handoff(self) -> Option<f64> {
        let checkpoint = self.episode.with(|episode| episode.latest())?;
        checkpoint.handoff_to(self.owner, self.seen.replace(Some(checkpoint)))
    }

    pub(super) fn flush(self) {
        self.sender.try_with(|sender| {
            sender.send_modify(|update| update.flush = update.flush.wrapping_add(1))
        });
    }

    pub(super) fn complete(self) {
        self.sender.try_with(|sender| {
            sender.send_modify(|update| {
                if let Some(position) = &mut update.position {
                    position.at = position.duration;
                    self.episode.with(|episode| {
                        if episode.owns_latest(self.owner) {
                            episode.record(Checkpoint {
                                owner: self.owner,
                                position: *position,
                                completed: true,
                            });
                        }
                    });
                }
                update.flush = update.flush.wrapping_add(1);
            })
        });
    }
}

pub(super) fn observe(
    session: SessionHandle,
    history: LocalHistoryItem,
    routed: Memo<bool>,
) -> Recorder {
    let state = session.with(|session| session.state.clone());
    let revision = Arc::new(AtomicU64::new(0));
    let login = state
        .settings()
        .account
        .as_ref()
        .map(|account| account.login_at);
    let episode =
        session
            .playback_progress()
            .episode((login, history.series_id.clone(), history.vid_index));
    let recorder = start_writer(state, history, revision.clone(), episode);
    effect(move || {
        if !routed.get() {
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

fn start_writer(
    state: AppState,
    history: LocalHistoryItem,
    revision: Arc<AtomicU64>,
    episode: Arc<EpisodeProgress>,
) -> Recorder {
    let (sender, receiver) = watch::channel(Update::default());
    let owner = uuid::Uuid::new_v4();
    // The source owner holds the only sender; its disposal flushes and ends the writer.
    let recorder = Recorder {
        sender: stored(sender),
        episode: stored(episode.clone()),
        owner,
        seen: stored(None),
    };
    let cloud_episode = episode.clone();
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
            let episode = cloud_episode.clone();
            async move {
                let _ordered = episode.cloud_write.lock().await;
                if !episode.owns_latest(owner) {
                    return Ok(());
                }
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
        let episode = episode.clone();
        async move {
            let _ordered = episode.local_write.lock().await;
            if !episode.owns_latest(owner) {
                return Ok(());
            }
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
mod tests {
    use super::*;

    #[test]
    fn another_route_can_seek_backward_and_finished_episode_restarts() {
        let home = uuid::Uuid::new_v4();
        let history = uuid::Uuid::new_v4();
        let a = Checkpoint {
            owner: history,
            position: ProgressPosition::new(52.0, 80.0).unwrap(),
            completed: false,
        };
        let b = Checkpoint {
            position: ProgressPosition::new(10.0, 80.0).unwrap(),
            ..a
        };
        assert_eq!(a.handoff_to(home, None), Some(52.0));
        assert_eq!(a.handoff_to(home, Some(a)), None);
        assert_eq!(b.handoff_to(home, Some(a)), Some(10.0));
        assert_eq!(b.handoff_to(history, None), None);
        assert_eq!(
            Checkpoint {
                completed: true,
                ..b
            }
            .handoff_to(home, Some(b)),
            Some(0.0)
        );
    }
}
