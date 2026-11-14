use std::{
    collections::{HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use hongguo_audio_api::PcmFormat;
use hongguo_codec_ffi::Backend;
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{DecodeDevice, FrameImage, HardwareKind, PresentationFrame};
use parking_lot::{Condvar, Mutex};

use crate::{IndexedSource, MonotonicClock, PcmRing};

#[path = "session_audio.rs"]
mod audio;
#[path = "session_buffering.rs"]
mod buffering;
#[path = "session_output.rs"]
mod output;
#[path = "session_video.rs"]
mod video;

const DUE_AHEAD_SECS: f64 = 0.008;

pub trait ReadAt: Send + Sync + 'static {
    fn length(&self) -> AppResult<u64>;
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Opening,
    Playing,
    Paused,
    Buffering,
    Seeking,
    Recovering,
    Ended,
    Failed,
    Closed,
}

#[derive(Clone, Debug)]
pub struct RecoveryLimits {
    pub attempts: u32,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub defer_decoder_binding: bool,
    pub pcm_format: PcmFormat,
    pub audio_capacity_frames: usize,
    pub video_queue_frames: usize,
    pub video_queue_bytes: usize,
    pub video_ahead_secs: f64,
    pub decoder_threads: u32,
    pub rate: f64,
    pub recovery: RecoveryLimits,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            defer_decoder_binding: false,
            pcm_format: PcmFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            audio_capacity_frames: 96_000,
            video_queue_frames: 4,
            video_queue_bytes: 64 * 1024 * 1024,
            video_ahead_secs: 0.75,
            decoder_threads: thread::available_parallelism()
                .map_or(1, |count| count.get().min(4) as u32),
            rate: 1.0,
            recovery: RecoveryLimits {
                attempts: 2,
                elapsed: Duration::from_secs(15),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SessionInfo {
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub has_audio: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SessionPosition {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub ended: bool,
    pub buffering: bool,
    /// The current movie position is in an explicit empty edit, not a video tail.
    pub video_gap: bool,
}

struct QueuedFrame {
    frame: Arc<PresentationFrame>,
    bytes: usize,
}

struct State {
    mode: PlaybackState,
    error: Option<String>,
    epoch: u64,
    target_secs: f64,
    rate: f64,
    desired_playing: bool,
    holding: bool,
    video_stall_started: Option<Instant>,
    video_work: (buffering::VideoWork, Instant),
    cancel: Arc<AtomicBool>,
    index: Option<Arc<IndexedSource>>,
    device: Option<Arc<dyn DecodeDevice>>,
    decoder_bound: bool,
    configured: bool,
    failed: HashSet<(Backend, u64, StreamKey)>,
    stream_key: Option<StreamKey>,
    recovery_attempts: u32,
    recovery_elapsed: Duration,
    recovery_started: Option<Instant>,
    queue: VecDeque<QueuedFrame>,
    queue_bytes: usize,
    current: Option<Arc<PresentationFrame>>,
    next_serial: u64,
    video_done: bool,
    audio_done: bool,
    audio_output_pending: bool,
    silent_clock: MonotonicClock,
}

#[derive(Clone)]
struct Job {
    epoch: u64,
    target_secs: f64,
    rate: f64,
    cancel: Arc<AtomicBool>,
    index: Arc<IndexedSource>,
    device: Option<Arc<dyn DecodeDevice>>,
    stream_key: StreamKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct StreamKey {
    codec: crate::TrackCodec,
    width: u32,
    height: u32,
    bit_depth: u8,
    configuration_hash: u64,
}

impl StreamKey {
    fn new(track: &crate::IndexedTrack) -> Self {
        let configuration_hash = track
            .extradata
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
            });
        let bit_depth = if track.codec == crate::TrackCodec::Hevc {
            track.extradata.get(17).map_or(8, |depth| (depth & 7) + 8)
        } else {
            8
        };
        Self {
            codec: track.codec,
            width: track.info.width,
            height: track.info.height,
            bit_depth,
            configuration_hash,
        }
    }
}

enum OutputCommand {
    Open,
    Start,
    Pause,
    Flush(u64),
    Close,
}

struct Shared {
    source: Arc<dyn ReadAt>,
    config: SessionConfig,
    state: Mutex<State>,
    changed: Condvar,
    pcm: Arc<PcmRing>,
    output: output::Port,
}

pub struct PlaybackSession {
    shared: Arc<Shared>,
}

impl PlaybackSession {
    pub fn open(source: Arc<dyn ReadAt>, config: SessionConfig) -> AppResult<Self> {
        Self::open_with_output(source, config, output::run)
    }

    fn open_with_output(
        source: Arc<dyn ReadAt>,
        config: SessionConfig,
        output_work: impl FnOnce(Arc<Shared>) + Send + 'static,
    ) -> AppResult<Self> {
        if config.video_queue_frames == 0
            || config.video_queue_bytes == 0
            || !config.video_ahead_secs.is_finite()
            || config.video_ahead_secs <= 0.0
            || config.decoder_threads == 0
        {
            return Err(AppError::Media(
                "invalid playback queue or thread budget".into(),
            ));
        }
        let pcm = Arc::new(PcmRing::new(
            config.pcm_format,
            config.audio_capacity_frames,
            1,
            0.0,
        )?);
        pcm.suspend();
        let clock = MonotonicClock::new(0.0, config.rate, Instant::now())?;
        let output = output::Port::new();
        let shared = Arc::new(Shared {
            source,
            pcm,
            output,
            state: Mutex::new(State {
                mode: PlaybackState::Opening,
                error: None,
                epoch: 1,
                target_secs: 0.0,
                rate: config.rate,
                desired_playing: false,
                holding: true,
                video_stall_started: None,
                video_work: (buffering::VideoWork::Waiting, Instant::now()),
                cancel: Arc::new(AtomicBool::new(false)),
                index: None,
                device: None,
                decoder_bound: !config.defer_decoder_binding,
                configured: false,
                failed: HashSet::new(),
                stream_key: None,
                recovery_attempts: 0,
                recovery_elapsed: Duration::ZERO,
                recovery_started: None,
                queue: VecDeque::new(),
                queue_bytes: 0,
                current: None,
                next_serial: 1,
                video_done: false,
                audio_done: false,
                audio_output_pending: false,
                silent_clock: clock,
            }),
            changed: Condvar::new(),
            config,
        });
        if let Err(error) = spawn("playback-output", shared.clone(), output_work) {
            shared.fail(&error);
            let _ = shared.output.send(OutputCommand::Close);
            return Err(error);
        }
        for (name, work) in [
            ("playback-video", video::run as fn(Arc<Shared>)),
            ("playback-audio", audio::run),
        ] {
            if let Err(error) = spawn(name, shared.clone(), work) {
                shared.fail(&error);
                let _ = shared.output.send(OutputCommand::Close);
                return Err(error);
            }
        }
        Ok(Self { shared })
    }

    pub fn pcm(&self) -> Arc<PcmRing> {
        self.shared.pcm.clone()
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

    pub fn state(&self) -> PlaybackState {
        self.shared.state.lock().mode
    }
    pub fn holding(&self) -> bool {
        self.shared.state.lock().holding
    }

    pub fn position(&self) -> AppResult<SessionPosition> {
        let mut state = self.shared.state.lock();
        self.shared.check_recovery_deadline(&mut state)?;
        state.check()?;
        self.shared.hold_starved_video(&mut state);
        let video_gap = self.shared.clear_empty_edit(&mut state);
        let mut position_secs = self.shared.position_locked(&state);
        let duration_secs = state
            .index
            .as_ref()
            .map_or(0.0, |index| index.duration_secs);
        if !state.holding
            && !state.audio_output_pending
            && state.desired_playing
            && state
                .index
                .as_ref()
                .is_some_and(|index| index.audio.is_some())
        {
            state.mode = if self.shared.pcm.is_starved() {
                PlaybackState::Buffering
            } else {
                PlaybackState::Playing
            };
        }
        if !state.audio_output_pending
            && state.video_done
            && state.audio_done
            && state.queue.is_empty()
            && duration_secs > 0.0
            && position_secs + DUE_AHEAD_SECS >= duration_secs
        {
            position_secs = duration_secs;
            state.mode = PlaybackState::Ended;
            state.silent_clock.set_running(false, Instant::now());
            let _ = self.shared.output.send(OutputCommand::Pause);
        }
        Ok(SessionPosition {
            position_secs,
            duration_secs,
            ended: state.mode == PlaybackState::Ended,
            video_gap,
            buffering: state.audio_output_pending
                || state.holding
                || matches!(
                    state.mode,
                    PlaybackState::Opening
                        | PlaybackState::Buffering
                        | PlaybackState::Seeking
                        | PlaybackState::Recovering
                ),
        })
    }

    pub fn frame_at(&self, position_secs: f64) -> AppResult<Option<Arc<PresentationFrame>>> {
        if !position_secs.is_finite() {
            return Err(AppError::Media("invalid presentation position".into()));
        }
        let mut state = self.shared.state.lock();
        self.shared.check_recovery_deadline(&mut state)?;
        state.check()?;
        if self.shared.clear_empty_edit(&mut state) {
            return Ok(None);
        }
        if state.holding && state.video_stall_started.is_none() {
            if state
                .current
                .as_ref()
                .is_none_or(|frame| frame.generation != state.epoch)
                && let Some(next) = state.queue.pop_front()
            {
                state.queue_bytes -= next.bytes;
                state.current = Some(next.frame);
            }
        } else {
            let position_secs = if state.video_stall_started.is_some() {
                self.shared.position_locked(&state)
            } else {
                position_secs
            };
            while state
                .queue
                .front()
                .is_some_and(|next| next.frame.pts_secs <= position_secs + DUE_AHEAD_SECS)
            {
                let next = state.queue.pop_front().expect("front checked");
                state.queue_bytes -= next.bytes;
                state.current = Some(next.frame);
            }
        }
        self.shared.hold_starved_video(&mut state);
        let frame = state
            .current
            .as_ref()
            .filter(|frame| frame.generation == state.epoch && self.shared.can_resume_video(&state))
            .cloned();
        self.shared.changed.notify_all();
        crate::budget::wake();
        Ok(frame)
    }

    pub fn presented(&self, generation: u64, serial: u64) -> AppResult<bool> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if !state.holding
            || state.epoch != generation
            || state
                .current
                .as_ref()
                .is_none_or(|frame| frame.serial != serial || frame.generation != generation)
            || !self.shared.can_resume_video(&state)
        {
            return Ok(false);
        }
        state.holding = false;
        self.shared.release_video_stall(&mut state);
        if !state.audio_output_pending
            && let Some(started) = state.recovery_started.take()
        {
            state.recovery_elapsed += started.elapsed();
        }
        let playing = state.desired_playing;
        state.mode = state.playback_mode();
        state.silent_clock.set_running(playing, Instant::now());
        if playing {
            let _ = self.shared.output.send(OutputCommand::Start);
        }
        self.shared.changed.notify_all();
        Ok(true)
    }

    pub fn seek(&self, position_secs: f64) -> AppResult<u64> {
        let mut state = self.shared.state.lock();
        state.check()?;
        self.shared
            .restart(&mut state, position_secs, PlaybackState::Seeking)
    }

    pub fn set_playing(&self, playing: bool) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.desired_playing == playing {
            return Ok(());
        }
        state.desired_playing = playing;
        let running = playing && !state.holding;
        state.silent_clock.set_running(running, Instant::now());
        if !state.holding {
            state.mode = state.playback_mode();
        }
        let _ = self.shared.output.send(if running {
            OutputCommand::Start
        } else {
            OutputCommand::Pause
        });
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn set_rate(&self, rate: f64) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.rate == rate {
            return Ok(());
        }
        let position = self.shared.position_locked(&state);
        state.silent_clock.set_rate(rate, Instant::now())?;
        state.rate = rate;
        self.shared
            .restart(&mut state, position, PlaybackState::Seeking)?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f32, muted: bool) -> AppResult<()> {
        self.shared.pcm.set_volume(volume, muted)
    }

    pub fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        let key = |device: &Option<Arc<dyn DecodeDevice>>| {
            device.as_ref().map(|device| {
                (
                    device.kind(),
                    device.device(),
                    device.surface(),
                    device.generation(),
                )
            })
        };
        let first = !state.configured;
        state.configured = true;
        if !state.decoder_bound {
            state.decoder_bound = true;
            state.device = device;
            if state
                .index
                .as_ref()
                .is_some_and(|index| index.audio.is_some())
            {
                let _ = self.shared.output.send(OutputCommand::Open);
            }
            self.shared.changed.notify_all();
            return Ok(());
        }
        if key(&device) == key(&state.device) {
            return Ok(());
        }
        let position = self.shared.position_locked(&state);
        state.device = device;
        if first {
            self.shared
                .restart(&mut state, position, PlaybackState::Seeking)?;
        } else {
            self.shared.recover_restart(&mut state, position)?;
        }
        Ok(())
    }

    pub fn reject_decoder(&self, detail: String) -> AppResult<()> {
        let state = self.shared.state.lock();
        state.check()?;
        let current = state
            .current
            .as_ref()
            .filter(|frame| frame.generation == state.epoch)
            .ok_or_else(|| AppError::Media("no current decoder frame to reject".into()))?;
        let FrameImage::Hardware(frame) = &current.image else {
            return Err(AppError::Media(
                "software presentation failed; no decoder fallback remains".into(),
            ));
        };
        let device = state.device.clone();
        let backend = match frame.handle().kind {
            HardwareKind::VideoToolbox => Backend::VideoToolbox,
            HardwareKind::D3d11 => Backend::D3d11 {
                device: device.as_ref().map_or(0, |device| device.device()),
            },
            HardwareKind::MediaCodec => Backend::MediaCodec {
                java_vm: device.as_ref().map_or(0, |device| device.device()),
                surface: device.as_ref().map_or(0, |device| device.surface()),
            },
        };
        let job = Job {
            epoch: state.epoch,
            target_secs: state.target_secs,
            rate: state.rate,
            cancel: state.cancel.clone(),
            index: state
                .index
                .clone()
                .ok_or_else(|| AppError::Media("decoder index unavailable".into()))?,
            device,
            stream_key: state
                .stream_key
                .ok_or_else(|| AppError::Media("decoder stream key unavailable".into()))?,
        };
        drop(state);
        video::recover(&self.shared, &job, backend, detail);
        self.shared.state.lock().check()
    }

    pub fn recover_presentation(&self, detail: String) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        let position = self.shared.position_locked(&state);
        self.shared.recover_restart(&mut state, position)?;
        drop(state);
        log::warn!("[playback] presentation recovery: {detail}");
        Ok(())
    }

    pub fn close(&self) {
        let mut state = self.shared.state.lock();
        state.mode = PlaybackState::Closed;
        state.cancel.store(true, Ordering::Release);
        state.queue.clear();
        state.current = None;
        self.shared.pcm.close();
        let _ = self.shared.output.send(OutputCommand::Close);
        self.shared.changed.notify_all();
        crate::budget::wake();
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.close();
    }
}

impl State {
    fn check(&self) -> AppResult<()> {
        if self.mode == PlaybackState::Closed {
            return Err(AppError::Cancelled);
        }
        if let Some(error) = &self.error {
            return Err(AppError::Media(error.clone()));
        }
        Ok(())
    }
}

impl Shared {
    fn position_locked(&self, state: &State) -> f64 {
        if state
            .index
            .as_ref()
            .is_some_and(|index| index.audio.is_some())
        {
            self.pcm.position()
        } else {
            state.silent_clock.position(Instant::now())
        }
    }

    fn restart(
        &self,
        state: &mut State,
        position_secs: f64,
        mode: PlaybackState,
    ) -> AppResult<u64> {
        if !position_secs.is_finite() || position_secs < 0.0 {
            return Err(AppError::Media("invalid seek position".into()));
        }
        let epoch = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| AppError::Media("playback epoch exhausted".into()))?;
        let position_secs = state.index.as_ref().map_or(position_secs, |index| {
            position_secs.min(index.duration_secs)
        });
        state.cancel.store(true, Ordering::Release);
        state.cancel = Arc::new(AtomicBool::new(false));
        state.epoch = epoch;
        state.target_secs = position_secs;
        state.holding = true;
        state.video_stall_started = None;
        state.video_work = (buffering::VideoWork::Waiting, Instant::now());
        state.mode = mode;
        state.queue.clear();
        state.queue_bytes = 0;
        state.video_done = false;
        state.audio_done = false;
        state.audio_output_pending = state
            .index
            .as_ref()
            .is_some_and(|index| index.audio.is_some());
        state.silent_clock.seek(position_secs, Instant::now())?;
        state.silent_clock.set_running(false, Instant::now());
        if state.index.as_ref().is_some_and(|index| {
            position_secs < index.duration_secs
                && index.video.timeline.movie_to_media(position_secs).is_none()
        }) {
            state.holding = false;
            let playing = state.desired_playing;
            state.mode = state.playback_mode();
            state.silent_clock.set_running(playing, Instant::now());
        }
        self.pcm.reset(epoch, position_secs);
        let _ = self.output.send(OutputCommand::Pause);
        let _ = self.output.send(OutputCommand::Flush(epoch));
        if !state.holding && state.desired_playing {
            let _ = self.output.send(OutputCommand::Start);
        }
        self.changed.notify_all();
        crate::budget::wake();
        Ok(epoch)
    }

    fn check_recovery_deadline(&self, state: &mut State) -> AppResult<()> {
        if state.recovery_started.is_some_and(|started| {
            state.recovery_elapsed + started.elapsed() >= self.config.recovery.elapsed
        }) {
            state.error = Some("decoder recovery time budget exhausted".into());
            state.mode = PlaybackState::Failed;
            state.cancel.store(true, Ordering::Release);
            self.pcm.close();
            let _ = self.output.send(OutputCommand::Pause);
            self.changed.notify_all();
        }
        state.check()
    }

    fn recover_restart(&self, state: &mut State, position: f64) -> AppResult<u64> {
        let elapsed = state.recovery_elapsed
            + state
                .recovery_started
                .map_or(Duration::ZERO, |started| started.elapsed());
        if state.recovery_attempts >= self.config.recovery.attempts
            || elapsed >= self.config.recovery.elapsed
        {
            state.error = Some("decoder recovery budget exhausted".into());
            state.mode = PlaybackState::Failed;
            state.cancel.store(true, Ordering::Release);
            self.pcm.close();
            let _ = self.output.send(OutputCommand::Pause);
            self.changed.notify_all();
            crate::budget::wake();
            return state.check().map(|()| state.epoch);
        }
        state.recovery_attempts += 1;
        state.recovery_started.get_or_insert_with(Instant::now);
        self.restart(state, position, PlaybackState::Recovering)
    }

    fn fail(&self, error: impl ToString) {
        let mut state = self.state.lock();
        if matches!(state.mode, PlaybackState::Closed | PlaybackState::Failed) {
            return;
        }
        state.error = Some(error.to_string());
        state.mode = PlaybackState::Failed;
        state.cancel.store(true, Ordering::Release);
        self.pcm.close();
        let _ = self.output.send(OutputCommand::Pause);
        self.changed.notify_all();
        crate::budget::wake();
    }

    fn job(&self, previous_epoch: u64) -> Option<Job> {
        let mut state = self.state.lock();
        loop {
            if matches!(state.mode, PlaybackState::Closed | PlaybackState::Failed) {
                return None;
            }
            if state.decoder_bound
                && state.epoch != previous_epoch
                && let Some(index) = &state.index
            {
                return Some(Job {
                    epoch: state.epoch,
                    target_secs: state.target_secs,
                    rate: state.rate,
                    cancel: state.cancel.clone(),
                    index: index.clone(),
                    device: state.device.clone(),
                    stream_key: state
                        .stream_key
                        .unwrap_or_else(|| StreamKey::new(&index.video)),
                });
            }
            self.changed.wait(&mut state);
        }
    }
}

fn spawn(
    name: &str,
    shared: Arc<Shared>,
    work: impl FnOnce(Arc<Shared>) + Send + 'static,
) -> AppResult<()> {
    let failure = shared.clone();
    let name = name.to_owned();
    thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(shared))).is_err() {
                failure.fail(format!("{name} panicked"));
            }
        })
        .map_err(|error| AppError::Io(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "session_buffering_tests.rs"]
mod buffering_tests;
