use crate::IndexedVideo;
use hongguo_common::{AppError, AppResult};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
#[path = "session/worker.rs"]
mod worker;

const QUEUE_DEPTH: usize = 4;
const DUE_AHEAD_MS: u64 = 8;
const MASK_CANCEL_INTERVAL: Duration = Duration::from_millis(100);

pub trait ReadAt: Send + Sync + 'static {
    fn length(&self) -> AppResult<u64>;
    /// Fill exactly length bytes or fail; inspect cancellation while waiting for data.
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct PixelFrame {
    pub serial: u64,
    pub generation: u64,
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Arc<[u8]>,
}

struct State {
    closed: bool,
    generation: u64,
    target_ms: u64,
    cancel: Arc<AtomicBool>,
    index: Option<Arc<IndexedVideo>>,
    info: Option<FrameInfo>,
    error: Option<String>,
    queue: VecDeque<Arc<PixelFrame>>,
    current: Option<Arc<PixelFrame>>,
    holding: bool,
    next_serial: u64,
}

impl State {
    fn check(&self) -> AppResult<()> {
        if self.closed {
            return Err(AppError::Cancelled);
        }
        if let Some(error) = &self.error {
            return Err(AppError::Media(error.clone()));
        }
        Ok(())
    }
}

struct Shared {
    source: Arc<dyn ReadAt>,
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct DirectSession {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl DirectSession {
    pub fn open(source: Arc<dyn ReadAt>) -> AppResult<Self> {
        let shared = Arc::new(Shared {
            source,
            state: Mutex::new(State {
                closed: false,
                generation: 1,
                target_ms: 0,
                cancel: Arc::new(AtomicBool::new(false)),
                index: None,
                info: None,
                error: None,
                queue: VecDeque::new(),
                current: None,
                holding: true,
                next_serial: 1,
            }),
            changed: Condvar::new(),
        });
        let owner = shared.clone();
        let worker = std::thread::Builder::new()
            .name("softdec-session".into())
            .spawn(move || worker::run(owner))
            .map_err(|error| AppError::Io(error.to_string()))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn info(&self) -> AppResult<Option<FrameInfo>> {
        let state = self.shared.lock();
        state.check()?;
        Ok(state.info)
    }

    /// Returns the current immutable frame, or the last frame due on the audio clock.
    pub fn frame_at(&self, position_ms: u64) -> AppResult<Option<Arc<PixelFrame>>> {
        let mut state = self.shared.lock();
        state.check()?;
        if state.holding {
            if state.current.is_none() {
                state.current = state.queue.pop_front();
            }
            let current = state.current.clone();
            self.shared.changed.notify_all();
            return Ok(current);
        }
        while state
            .queue
            .front()
            .is_some_and(|frame| frame.pts_ms <= position_ms.saturating_add(DUE_AHEAD_MS))
        {
            state.current = state.queue.pop_front();
        }
        let current = state.current.clone();
        self.shared.changed.notify_all();
        Ok(current)
    }

    pub fn seek(&self, position_ms: u64) -> AppResult<u64> {
        let mut state = self.shared.lock();
        state.check()?;
        let generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| AppError::Internal("softdec generation exhausted".into()))?;
        state.cancel.store(true, Ordering::Release);
        state.cancel = Arc::new(AtomicBool::new(false));
        state.generation = generation;
        state.target_ms = position_ms;
        state.queue.clear();
        state.current = None;
        state.holding = true;
        self.shared.changed.notify_all();
        Ok(generation)
    }

    /// True exactly once when the current generation's held frame was displayed.
    pub fn presented(&self, generation: u64, serial: u64) -> AppResult<bool> {
        let mut state = self.shared.lock();
        state.check()?;
        if !state.holding
            || state.generation != generation
            || state
                .current
                .as_ref()
                .is_none_or(|frame| frame.serial != serial || frame.generation != generation)
        {
            return Ok(false);
        }
        state.holding = false;
        self.shared.changed.notify_all();
        Ok(true)
    }

    pub fn holding(&self) -> bool {
        let state = self.shared.lock();
        !state.closed && state.holding
    }

    /// Audio reads call this on their IO worker after source registration.
    pub fn mask_video_tracks(&self, offset: u64, bytes: &mut [u8]) -> AppResult<()> {
        self.mask_video_tracks_cancellable(offset, bytes, || false)
    }

    pub fn mask_video_tracks_cancellable(
        &self,
        offset: u64,
        bytes: &mut [u8],
        cancelled: impl Fn() -> bool,
    ) -> AppResult<()> {
        let index = loop {
            if cancelled() {
                return Err(AppError::Cancelled);
            }
            let state = self.shared.lock();
            state.check()?;
            if let Some(index) = &state.index {
                break index.clone();
            }
            drop(
                self.shared
                    .changed
                    .wait_timeout(state, MASK_CANCEL_INTERVAL)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        };
        index.mask_video_tracks(offset, bytes);
        if cancelled() {
            return Err(AppError::Cancelled);
        }
        self.shared.lock().check()
    }

    pub fn close(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        state.cancel.store(true, Ordering::Release);
        state.queue.clear();
        state.current = None;
        self.shared.changed.notify_all();
    }
}

impl Drop for DirectSession {
    fn drop(&mut self) {
        self.close();
        // Cancellation wakes reads/backpressure; never join a blocking worker on a UI thread.
        drop(self.worker.take());
    }
}
