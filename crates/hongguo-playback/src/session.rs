use crate::{IndexedSource, MonotonicClock};
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{DecodeDevice, PresentationFrame};
use hongguo_native_clock::{Clock, Control};
use parking_lot::{Condvar, Mutex};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};

mod control;
mod decoder;
#[cfg(test)]
mod hardware_tests;
#[cfg(test)]
mod tests;
mod timing;
mod types;
mod worker;
pub use types::{ReadAt, RecoveryLimits, SessionConfig, SessionInfo, SessionPosition};

const DUE_AHEAD_SECS: f64 = 0.008;

struct State {
    closed: bool,
    error: Option<String>,
    epoch: u64,
    target: f64,
    position: f64,
    control: Control,
    holding: bool,
    presented: bool,
    decoder_bound: bool,
    device: Option<Arc<dyn DecodeDevice>>,
    hardware_disabled: bool,
    cancel: Arc<AtomicBool>,
    index: Option<Arc<IndexedSource>>,
    clock: Option<Arc<dyn Clock>>,
    silent: MonotonicClock,
    audio_tail: bool,
    queue: VecDeque<Arc<PresentationFrame>>,
    bytes: usize,
    current: Option<Arc<PresentationFrame>>,
    next_serial: u64,
    video_done: bool,
    video_gap: bool,
    buffering: bool,
    recoveries: u32,
    stalled_at: Option<Instant>,
}

struct Shared {
    source: Arc<dyn ReadAt>,
    metadata_cancel: AtomicBool,
    config: SessionConfig,
    state: Mutex<State>,
    changed: Condvar,
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

pub struct PlaybackSession {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl PlaybackSession {
    pub fn open(source: Arc<dyn ReadAt>, config: SessionConfig) -> AppResult<Self> {
        if config.video_queue_frames == 0
            || config.video_queue_bytes == 0
            || !config.video_ahead_secs.is_finite()
            || config.video_ahead_secs <= 0.0
            || config.decoder_threads == 0
            || config.recovery.elapsed.is_zero()
        {
            return Err(AppError::Media(
                "invalid video codec session configuration".into(),
            ));
        }
        let silent = MonotonicClock::new(config.start_position_secs, config.rate, Instant::now())?;
        let bound = !config.defer_decoder_binding;
        let rate = config.rate;
        let start = config.start_position_secs;
        let shared = Arc::new(Shared {
            source,
            metadata_cancel: AtomicBool::new(false),
            config,
            state: Mutex::new(State {
                closed: false,
                error: None,
                epoch: 1,
                target: start,
                position: start,
                control: Control {
                    rate,
                    ..Default::default()
                },
                holding: true,
                presented: false,
                decoder_bound: bound,
                device: None,
                hardware_disabled: false,
                cancel: Arc::new(AtomicBool::new(false)),
                index: None,
                clock: None,
                silent,
                audio_tail: false,
                queue: VecDeque::new(),
                bytes: 0,
                current: None,
                next_serial: 1,
                video_done: false,
                video_gap: false,
                buffering: true,
                recoveries: 0,
                stalled_at: None,
            }),
            changed: Condvar::new(),
        });
        let decoder = shared.clone();
        let worker = std::thread::Builder::new()
            .name("video-codec-extension".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker::run(&decoder)
                }));
                match result {
                    Ok(Err(error)) => decoder.fail(error.to_string()),
                    Err(_) => decoder.fail("video codec worker panicked".into()),
                    Ok(Ok(())) => {}
                }
            })
            .map_err(|error| AppError::Io(error.to_string()))?;
        Ok(Self {
            shared,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn info(&self) -> AppResult<Option<SessionInfo>> {
        let state = self.shared.state.lock();
        state.check()?;
        Ok(state.index.as_ref().map(|index| SessionInfo {
            width: index.video.info.width,
            height: index.video.info.height,
            duration_secs: index.duration_secs,
            has_audio: index.audio.is_some(),
        }))
    }

    pub fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        let changed = state
            .device
            .as_ref()
            .map(|device| (device.kind(), device.device(), device.generation()))
            != device
                .as_ref()
                .map(|device| (device.kind(), device.device(), device.generation()));
        if changed && state.decoder_bound {
            self.shared.refresh(&mut state)?;
            let target = state.position;
            self.shared.restart(&mut state, target)?;
        }
        if changed {
            state.hardware_disabled = false;
        }
        state.device = device;
        state.decoder_bound = true;
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn configure_clock(&self, clock: Arc<dyn Clock>) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.clock.is_some() {
            return Err(AppError::Media(
                "native playback clock already attached".into(),
            ));
        }
        if state.target > 0.0 {
            clock.seek(state.target).map_err(AppError::Media)?;
        }
        state.clock = Some(clock);
        self.shared.refresh(&mut state)?;
        Ok(())
    }

    pub fn has_native_clock(&self) -> bool {
        self.shared.state.lock().clock.is_some()
    }

    pub fn frame_at(&self, position: f64) -> AppResult<Option<Arc<PresentationFrame>>> {
        if !position.is_finite() || position < 0.0 {
            return Err(AppError::Media("invalid video frame position".into()));
        }
        let mut state = self.shared.state.lock();
        state.check()?;
        self.shared.refresh(&mut state)?;
        if state.video_gap {
            state.current = None;
            self.shared.changed.notify_all();
            return Ok(None);
        }
        if state.holding {
            if state.current.is_none() {
                state.current = state.pop();
            }
        } else {
            while state
                .queue
                .front()
                .is_some_and(|frame| frame.pts_secs <= position + DUE_AHEAD_SECS)
            {
                state.current = state.pop();
            }
        }
        let frame = state.current.clone();
        self.shared.changed.notify_all();
        Ok(frame)
    }

    pub fn presented(&self, generation: u64, serial: u64) -> AppResult<bool> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.epoch != generation
            || state.presented
            || state
                .current
                .as_ref()
                .is_none_or(|frame| frame.generation != generation || frame.serial != serial)
        {
            return Ok(false);
        }
        state.presented = true;
        self.shared.refresh(&mut state)?;
        self.shared.changed.notify_all();
        Ok(true)
    }

    pub fn position(&self) -> AppResult<SessionPosition> {
        let mut state = self.shared.state.lock();
        state.check()?;
        self.shared.refresh(&mut state)?;
        let duration = state
            .index
            .as_ref()
            .map_or(0.0, |index| index.duration_secs);
        self.shared.changed.notify_all();
        Ok(SessionPosition {
            position_secs: state.position,
            duration_secs: duration,
            ended: duration > 0.0 && state.position >= duration,
            buffering: state.buffering,
            video_gap: state.video_gap,
        })
    }

    pub fn holding(&self) -> bool {
        let mut state = self.shared.state.lock();
        if let Err(error) = self.shared.refresh(&mut state) {
            state.error = Some(error.to_string());
        }
        state.holding
    }

    pub fn reject_decoder(&self, detail: String) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.hardware_disabled || state.device.is_none() {
            return Err(AppError::Media(detail));
        }
        self.shared.refresh(&mut state)?;
        state.hardware_disabled = true;
        let target = state.position;
        log::warn!("[video] native surface rejected; switching to software: {detail}");
        self.shared.restart(&mut state, target)
    }

    pub fn close(&self) {
        self.shared.metadata_cancel.store(true, Ordering::Release);
        let clock = {
            let mut state = self.shared.state.lock();
            state.closed = true;
            state.cancel.store(true, Ordering::Release);
            state.queue.clear();
            state.current = None;
            state.bytes = 0;
            state.clock.take()
        };
        if let Some(clock) = clock {
            clock.close();
        }
        crate::budget::wake();
        self.shared.changed.notify_all();
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.close();
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl State {
    fn pop(&mut self) -> Option<Arc<PresentationFrame>> {
        let frame = self.queue.pop_front()?;
        self.bytes -= frame_bytes(&frame);
        Some(frame)
    }
}

fn frame_bytes(frame: &PresentationFrame) -> usize {
    match &frame.image {
        hongguo_media_types::FrameImage::Cpu(frame) => {
            frame.planes.iter().map(|plane| plane.data.len()).sum()
        }
        hongguo_media_types::FrameImage::Hardware(frame) => {
            let descriptor = frame.descriptor();
            descriptor
                .width
                .saturating_mul(descriptor.height)
                .saturating_mul(3)
        }
    }
}

impl Shared {
    fn fail(&self, error: String) {
        let mut state = self.state.lock();
        if state.closed {
            return;
        }
        state.error = Some(error);
        if let Some(clock) = &state.clock {
            let _ = clock.control(Control {
                playing: false,
                ..state.control
            });
        }
        state.cancel.store(true, Ordering::Release);
        self.changed.notify_all();
        crate::budget::wake();
    }
}
