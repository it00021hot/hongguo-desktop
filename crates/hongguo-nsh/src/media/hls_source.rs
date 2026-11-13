use hongguo_media::mp4::fragment_index::FragmentIndex;
use hongguo_protocol::grow::{self, GrowthState};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) const TARGET_DURATION_SECONDS: u32 = 6;

#[derive(Debug)]
pub(super) enum Failure {
    Cancelled,
    Failed(String),
    Invalid(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("media read cancelled"),
            Self::Failed(message) | Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

#[derive(Clone, Default)]
struct Cached {
    index: FragmentIndex,
    committed_bytes: u64,
    completed: bool,
}

pub(super) struct Published {
    pub index: FragmentIndex,
    pub completed: bool,
}

#[derive(Clone)]
pub(super) struct Growing {
    pub path: PathBuf,
    cache: Arc<Mutex<Cached>>,
}

impl Growing {
    pub fn wait_for_available(
        &self,
        target: f64,
        revoked: &AtomicBool,
        cancelled: &AtomicBool,
    ) -> Result<(), Failure> {
        loop {
            interrupted(revoked, cancelled)?;
            let before = state(&self.path)?;
            let published = self.snapshot(revoked, cancelled, false)?;
            let duration: f64 = published
                .index
                .fragments()
                .iter()
                .map(|fragment| fragment.duration_seconds())
                .sum();
            if target < duration {
                return Ok(());
            }
            if published.completed {
                return Err(Failure::Invalid(
                    "seek target exceeds completed media".into(),
                ));
            }
            grow::wait_for_change(&self.path, before.committed_bytes, || {
                revoked.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire)
            })
            .map_err(|_| Failure::Cancelled)?;
        }
    }

    pub fn available_duration(&self) -> Result<f64, Failure> {
        state(&self.path)?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| Failure::Invalid("fragment index lock poisoned".into()))?;
        Ok(cache
            .index
            .fragments()
            .iter()
            .map(|fragment| fragment.duration_seconds())
            .sum())
    }

    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            cache: Arc::default(),
        }
    }

    pub fn snapshot(
        &self,
        revoked: &AtomicBool,
        cancelled: &AtomicBool,
        wait: bool,
    ) -> Result<Published, Failure> {
        loop {
            interrupted(revoked, cancelled)?;
            let current = state(&self.path)?;
            let completed = current.state == GrowthState::Done;
            let previous = self
                .cache
                .lock()
                .map_err(|_| Failure::Invalid("fragment index lock poisoned".into()))?
                .clone();
            let next = if previous.committed_bytes == current.committed_bytes
                && previous.completed == completed
            {
                previous
            } else {
                let mut index = previous.index;
                let mut file =
                    File::open(&self.path).map_err(|error| Failure::Invalid(error.to_string()))?;
                index
                    .scan(&mut file, current.committed_bytes, completed)
                    .map_err(|error| Failure::Invalid(error.to_string()))?;
                validate(&index)?;
                let next = Cached {
                    index,
                    committed_bytes: current.committed_bytes,
                    completed,
                };
                interrupted(revoked, cancelled)?;
                state(&self.path)?;
                let mut cache = self
                    .cache
                    .lock()
                    .map_err(|_| Failure::Invalid("fragment index lock poisoned".into()))?;
                if (next.committed_bytes, next.completed)
                    >= (cache.committed_bytes, cache.completed)
                {
                    *cache = next.clone();
                }
                next
            };
            if !wait || completed || !next.index.fragments().is_empty() {
                return Ok(Published {
                    index: next.index,
                    completed,
                });
            }
            grow::wait_for_change(&self.path, current.committed_bytes, || {
                revoked.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire)
            })
            .map_err(|_| Failure::Cancelled)?;
        }
    }
}

pub(super) fn state(path: &Path) -> Result<grow::GrowthSnapshot, Failure> {
    let snapshot = grow::snapshot(path)
        .ok_or_else(|| Failure::Invalid("growth session is no longer registered".into()))?;
    match &snapshot.state {
        GrowthState::Failed(message) => Err(Failure::Failed(message.clone())),
        GrowthState::Cancelled => Err(Failure::Cancelled),
        _ => Ok(snapshot),
    }
}

fn interrupted(revoked: &AtomicBool, cancelled: &AtomicBool) -> Result<(), Failure> {
    if revoked.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire) {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn validate(index: &FragmentIndex) -> Result<(), Failure> {
    for fragment in index.fragments().iter().copied().chain(index.pending()) {
        if fragment.duration_ticks
            > u64::from(TARGET_DURATION_SECONDS) * u64::from(fragment.timescale)
        {
            return Err(Failure::Invalid(
                "encoded GOP exceeds the fixed HLS target duration".into(),
            ));
        }
    }
    Ok(())
}
