use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use hongguo_audio_api::{PcmFormat, PcmSource, ReadResult};
use hongguo_common::{AppError, AppResult};
use parking_lot::{Condvar, Mutex};

const MAX_RENDERED_SEGMENTS: usize = 1024;
const MAX_PCM_CHUNKS: usize = 1024;
const PENDING_EPOCH_SHIFT: u32 = 32;
const PENDING_FRAMES_MASK: u64 = u32::MAX as u64;

pub struct PcmChunk {
    pub samples: Vec<f32>,
    pub media_start_secs: f64,
    pub media_duration_secs: f64,
}

struct QueuedChunk {
    chunk: PcmChunk,
    offset_frames: usize,
}

#[derive(Clone, Copy)]
struct RenderedSegment {
    device_start: u64,
    frames: usize,
    media_start_secs: f64,
    media_secs_per_frame: f64,
}

struct State {
    epoch: u64,
    closed: bool,
    queued_frames: usize,
    queue: VecDeque<QueuedChunk>,
    retired: VecDeque<PcmChunk>,
    rendered: VecDeque<RenderedSegment>,
    device_cursor: u64,
    played_frames: u64,
    media_position_secs: f64,
}

pub struct PcmRing {
    format: PcmFormat,
    capacity_frames: usize,
    state: Mutex<State>,
    changed: Condvar,
    gain: AtomicU32,
    starved: AtomicBool,
    active: AtomicBool,
    epoch: AtomicU64,
    pending_silence: AtomicU64,
}

impl PcmRing {
    pub fn new(
        format: PcmFormat,
        capacity_frames: usize,
        epoch: u64,
        position_secs: f64,
    ) -> AppResult<Self> {
        if format.sample_rate == 0
            || format.channels == 0
            || capacity_frames == 0
            || !position_secs.is_finite()
        {
            return Err(AppError::Media(
                "invalid PCM ring format or capacity".into(),
            ));
        }
        Ok(Self {
            format,
            capacity_frames,
            state: Mutex::new(State {
                epoch,
                closed: false,
                queued_frames: 0,
                queue: VecDeque::with_capacity(MAX_PCM_CHUNKS),
                retired: VecDeque::with_capacity(MAX_PCM_CHUNKS),
                rendered: VecDeque::with_capacity(MAX_RENDERED_SEGMENTS),
                device_cursor: 0,
                played_frames: 0,
                media_position_secs: position_secs,
            }),
            changed: Condvar::new(),
            gain: AtomicU32::new(1.0f32.to_bits()),
            starved: AtomicBool::new(false),
            active: AtomicBool::new(true),
            epoch: AtomicU64::new(epoch),
            pending_silence: AtomicU64::new(epoch_key(epoch)),
        })
    }

    pub fn push(&self, epoch: u64, chunk: PcmChunk, cancelled: &AtomicBool) -> AppResult<()> {
        let channels = usize::from(self.format.channels);
        let frames = chunk.samples.len() / channels;
        if frames == 0
            || !chunk.samples.len().is_multiple_of(channels)
            || frames > self.capacity_frames
            || !chunk.media_start_secs.is_finite()
            || !chunk.media_duration_secs.is_finite()
            || chunk.media_duration_secs <= 0.0
        {
            return Err(AppError::Media(
                "invalid PCM chunk or capacity exceeded".into(),
            ));
        }
        let mut state = self.state.lock();
        state.retired.clear();
        while !state.closed
            && state.epoch == epoch
            && !cancelled.load(Ordering::Acquire)
            && (state.queued_frames + frames > self.capacity_frames
                || state.queue.len() >= MAX_PCM_CHUNKS)
        {
            self.changed.wait(&mut state);
        }
        if state.closed || state.epoch != epoch || cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        state.retired.clear();
        state.queued_frames += frames;
        state.queue.push_back(QueuedChunk {
            chunk,
            offset_frames: 0,
        });
        Ok(())
    }

    pub fn reset(&self, epoch: u64, position_secs: f64) {
        self.active.store(false, Ordering::Release);
        let mut state = self.state.lock();
        self.active.store(false, Ordering::Release);
        state.epoch = epoch;
        state.queue.clear();
        state.retired.clear();
        state.rendered.clear();
        state.queued_frames = 0;
        state.device_cursor = 0;
        state.played_frames = 0;
        state.media_position_secs = position_secs;
        self.starved.store(false, Ordering::Release);
        self.pending_silence
            .store(epoch_key(epoch), Ordering::Release);
        self.epoch.store(epoch, Ordering::Release);
        self.changed.notify_all();
    }

    pub fn suspend(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub fn activate(&self, epoch: u64) {
        let state = self.state.lock();
        if state.epoch == epoch && !state.closed {
            self.active.store(true, Ordering::Release);
        }
    }

    pub fn position(&self) -> f64 {
        self.state.lock().media_position_secs
    }
    /// Control-thread position queries must retain a final update even if a
    /// realtime callback could not acquire the ring's producer lock.
    pub(crate) fn synchronize_position(&self, epoch: u64, device_frames: u64) {
        advance_position(&mut self.state.lock(), epoch, device_frames);
    }
    pub fn is_starved(&self) -> bool {
        self.starved.load(Ordering::Acquire)
    }
    pub fn queued_frames(&self) -> usize {
        self.state.lock().queued_frames
    }
    pub fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }
    pub fn format(&self) -> PcmFormat {
        self.format
    }

    pub fn set_volume(&self, volume: f32, muted: bool) -> AppResult<()> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(AppError::Media("invalid PCM volume".into()));
        }
        self.gain.store(
            if muted { 0.0f32 } else { volume }.to_bits(),
            Ordering::Release,
        );
        Ok(())
    }

    pub fn close(&self) {
        self.active.store(false, Ordering::Release);
        let mut state = self.state.lock();
        state.closed = true;
        state.queue.clear();
        state.retired.clear();
        state.queued_frames = 0;
        self.changed.notify_all();
    }
}

impl PcmSource for PcmRing {
    fn read(&self, output: &mut [f32]) -> ReadResult {
        output.fill(0.0);
        let channels = usize::from(self.format.channels);
        let requested = output.len() / channels;
        let epoch = self.epoch.load(Ordering::Acquire);
        if !self.active.load(Ordering::Acquire) {
            return ReadResult {
                media_frames: 0,
                epoch,
            };
        }
        let Some(mut state) = self.state.try_lock() else {
            let _ =
                self.pending_silence
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                        (pending & !PENDING_FRAMES_MASK == epoch_key(epoch)).then_some(
                            epoch_key(epoch)
                                | (pending & PENDING_FRAMES_MASK)
                                    .saturating_add(requested as u64)
                                    .min(PENDING_FRAMES_MASK),
                        )
                    });
            self.starved.store(true, Ordering::Release);
            return ReadResult {
                media_frames: 0,
                epoch,
            };
        };
        if !self.active.load(Ordering::Acquire) {
            return ReadResult {
                media_frames: 0,
                epoch: state.epoch,
            };
        }
        let missed = self
            .pending_silence
            .swap(epoch_key(state.epoch), Ordering::AcqRel);
        if missed & !PENDING_FRAMES_MASK == epoch_key(state.epoch) {
            state.device_cursor = state
                .device_cursor
                .saturating_add(missed & PENDING_FRAMES_MASK);
        }
        let mut copied = 0usize;
        while !state.closed && copied < requested && state.rendered.len() < MAX_RENDERED_SEGMENTS {
            let Some(mut queued) = state.queue.pop_front() else {
                break;
            };
            let total = queued.chunk.samples.len() / channels;
            let frames = (total - queued.offset_frames).min(requested - copied);
            let media_secs_per_frame = queued.chunk.media_duration_secs / total as f64;
            output[copied * channels..(copied + frames) * channels].copy_from_slice(
                &queued.chunk.samples
                    [queued.offset_frames * channels..(queued.offset_frames + frames) * channels],
            );
            let device_start = state.device_cursor + copied as u64;
            state.rendered.push_back(RenderedSegment {
                device_start,
                frames,
                media_start_secs: queued.chunk.media_start_secs
                    + queued.offset_frames as f64 * media_secs_per_frame,
                media_secs_per_frame,
            });
            queued.offset_frames += frames;
            state.queued_frames -= frames;
            copied += frames;
            if queued.offset_frames < total {
                state.queue.push_front(queued);
            } else {
                state.retired.push_back(queued.chunk);
            }
        }
        state.device_cursor = state.device_cursor.saturating_add(requested as u64);
        let gain = f32::from_bits(self.gain.load(Ordering::Acquire));
        self.starved.store(copied < requested, Ordering::Release);
        if gain != 1.0 {
            for sample in &mut output[..copied * channels] {
                *sample *= gain;
            }
        }
        self.changed.notify_all();
        ReadResult {
            media_frames: copied as u32,
            epoch: state.epoch,
        }
    }

    fn played(&self, epoch: u64, device_frames: u64) {
        let Some(mut state) = self.state.try_lock() else {
            return;
        };
        advance_position(&mut state, epoch, device_frames);
    }
}

fn advance_position(state: &mut State, epoch: u64, device_frames: u64) {
    if epoch != state.epoch || device_frames < state.played_frames {
        return;
    }
    let device_frames = device_frames.min(state.device_cursor);
    state.played_frames = device_frames;
    while let Some(segment) = state.rendered.front().copied() {
        if device_frames < segment.device_start {
            break;
        }
        let played = (device_frames - segment.device_start).min(segment.frames as u64);
        state.media_position_secs =
            segment.media_start_secs + played as f64 * segment.media_secs_per_frame;
        if played < segment.frames as u64 {
            break;
        }
        state.rendered.pop_front();
    }
}

fn epoch_key(epoch: u64) -> u64 {
    (epoch as u32 as u64) << PENDING_EPOCH_SHIFT
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
