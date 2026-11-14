use super::*;
use hongguo_audio_api::{AudioError, AudioErrorKind, AudioOutput};

const HEARTBEAT: Duration = Duration::from_millis(250);
const REOPEN_PAUSE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Default)]
struct Intent {
    version: u64,
    open: bool,
    playing: bool,
    epoch: u64,
    closed: bool,
}

pub(super) struct Port {
    intent: Mutex<Intent>,
    changed: Condvar,
}

impl Port {
    pub(super) fn new() -> Self {
        Self {
            intent: Mutex::new(Intent::default()),
            changed: Condvar::new(),
        }
    }
    pub(super) fn send(&self, command: OutputCommand) -> AppResult<()> {
        let mut intent = self.intent.lock();
        if intent.closed {
            return Err(AppError::Cancelled);
        }
        match command {
            OutputCommand::Open => intent.open = true,
            OutputCommand::Start => intent.playing = true,
            OutputCommand::Pause => intent.playing = false,
            OutputCommand::Flush(epoch) => {
                intent.epoch = epoch;
                intent.playing = false;
            }
            OutputCommand::Close => intent.closed = true,
        }
        intent.version = intent
            .version
            .checked_add(1)
            .ok_or_else(|| AppError::Media("audio control generation exhausted".into()))?;
        self.changed.notify_one();
        Ok(())
    }

    fn next(&self, version: u64, timeout: Option<Duration>) -> Intent {
        let mut intent = self.intent.lock();
        if let Some(timeout) = timeout {
            if !intent.closed && intent.version == version {
                self.changed.wait_for(&mut intent, timeout);
            }
        } else {
            while !intent.closed && intent.version == version {
                self.changed.wait(&mut intent);
            }
        }
        *intent
    }

    fn wait_to_reopen(&self, delay: Duration) -> bool {
        let deadline = Instant::now() + delay;
        let mut intent = self.intent.lock();
        while !intent.closed && Instant::now() < deadline {
            self.changed.wait_for(
                &mut intent,
                deadline.saturating_duration_since(Instant::now()),
            );
        }
        !intent.closed
    }
}

pub(super) fn run(shared: Arc<Shared>) {
    run_with(shared, |shared| {
        hongguo_audio::Output::open(shared.config.pcm_format, shared.pcm.clone())
            .map(|output| Box::new(output) as Box<dyn AudioOutput>)
    });
}

fn run_with(
    shared: Arc<Shared>,
    factory: impl FnMut(&Shared) -> Result<Box<dyn AudioOutput>, AudioError>,
) {
    Runtime {
        shared,
        factory,
        output: None,
        version: 0,
        applied_epoch: None,
        running: false,
        next_check: Instant::now(),
    }
    .run();
}

struct Runtime<F> {
    shared: Arc<Shared>,
    factory: F,
    output: Option<Box<dyn AudioOutput>>,
    version: u64,
    applied_epoch: Option<u64>,
    running: bool,
    next_check: Instant,
}

impl<F: FnMut(&Shared) -> Result<Box<dyn AudioOutput>, AudioError>> Runtime<F> {
    fn run(&mut self) {
        while self.check_deadline() {
            let intent = self.shared.output.next(self.version, self.timeout());
            self.version = intent.version;
            if intent.closed || !self.check_deadline() {
                return;
            }
            let result = self.apply(intent).and_then(|()| {
                if self.running && Instant::now() >= self.next_check {
                    self.query_position()?;
                    self.next_check = Instant::now() + HEARTBEAT;
                }
                Ok(())
            });
            if let Err(error) = result {
                if !self.recover(error) {
                    return;
                }
                let delay = self.timeout().unwrap_or(REOPEN_PAUSE).min(REOPEN_PAUSE);
                if !self.shared.output.wait_to_reopen(delay) {
                    return;
                }
            }
        }
    }

    fn timeout(&self) -> Option<Duration> {
        let state = self.shared.state.lock();
        let recovery = state.recovery_started.map(|started| {
            self.shared
                .config
                .recovery
                .elapsed
                .saturating_sub(state.recovery_elapsed + started.elapsed())
        });
        let heartbeat = self
            .running
            .then(|| self.next_check.saturating_duration_since(Instant::now()));
        match (recovery, heartbeat) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        }
    }

    fn check_deadline(&self) -> bool {
        self.shared
            .check_recovery_deadline(&mut self.shared.state.lock())
            .is_ok()
    }

    fn apply(&mut self, intent: Intent) -> Result<(), AudioError> {
        if intent.open && self.output.is_none() {
            self.output = Some((self.factory)(&self.shared)?);
            if !self.check_deadline() {
                return Ok(());
            }
            if self.output.as_ref().expect("opened output").format()
                != self.shared.config.pcm_format
            {
                return Err(AudioError::invalid_format(
                    "audio output changed the negotiated PCM format",
                ));
            }
        }
        if self.output.is_none() {
            return Ok(());
        }
        let epoch = self.shared.state.lock().epoch;
        if self.applied_epoch != Some(epoch) {
            self.output.as_mut().expect("opened output").flush(epoch)?;
            if !self.check_deadline() {
                return Ok(());
            }
            self.applied_epoch = Some(epoch);
            self.running = false;
            activate_unless_video_stalled(&self.shared, epoch);
        }
        let playing = {
            let state = self.shared.state.lock();
            intent.playing && state.desired_playing && !state.holding && state.epoch == epoch
        };
        if self.running != playing {
            if playing {
                // A paused device can disappear or stop being the default. Check
                // before Start so no PCM escapes to the old route on resume.
                self.query_position()?;
                self.output.as_mut().expect("opened output").start()?;
                self.next_check = Instant::now() + HEARTBEAT;
            } else {
                self.output.as_mut().expect("opened output").pause()?;
                self.query_position()?;
            }
            self.running = playing;
        }
        if !self.check_deadline() {
            return Ok(());
        }
        self.finish_ready(epoch);
        Ok(())
    }

    fn finish_ready(&self, epoch: u64) {
        let mut state = self.shared.state.lock();
        if state.epoch != epoch
            || !state.audio_output_pending
            || !(self.running || !state.desired_playing)
        {
            return;
        }
        state.audio_output_pending = false;
        if !state.holding {
            if let Some(started) = state.recovery_started.take() {
                state.recovery_elapsed += started.elapsed();
            }
            state.mode = state.playback_mode();
        }
        self.shared.changed.notify_all();
    }

    fn query_position(&self) -> Result<(), AudioError> {
        let position = self.output.as_ref().expect("opened output").position()?;
        self.shared
            .pcm
            .synchronize_position(position.epoch, position.device_frames);
        Ok(())
    }

    fn recover(&mut self, mut error: AudioError) -> bool {
        self.shared.pcm.suspend();
        self.running = false;
        self.applied_epoch = None;
        if let Some(mut output) = self.output.take() {
            if let Err(retire) = output.pause()
                && retire.kind() != AudioErrorKind::Device
            {
                error = retire;
            }
            match output.position() {
                Ok(position) => self
                    .shared
                    .pcm
                    .synchronize_position(position.epoch, position.device_frames),
                Err(retire) if retire.kind() != AudioErrorKind::Device => error = retire,
                Err(_) => {}
            }
            // Native close joins callbacks. Snapshot consumption only after those
            // callbacks cannot advance the old epoch any further.
            drop(output);
        }
        if error.kind() != AudioErrorKind::Device {
            self.shared.fail(error);
            return false;
        }
        let mut state = self.shared.state.lock();
        if self.shared.check_recovery_deadline(&mut state).is_err() {
            return false;
        }
        let position = self.shared.position_locked(&state);
        state.audio_output_pending = true;
        if self.shared.recover_restart(&mut state, position).is_err() {
            return false;
        }
        log::warn!(
            "[playback] audio output failed; reopening the same source at {position:.3}: {error}"
        );
        true
    }
}

impl State {
    pub(super) fn playback_mode(&self) -> PlaybackState {
        if self.audio_output_pending {
            if self.recovery_started.is_some() {
                PlaybackState::Recovering
            } else if self.mode == PlaybackState::Seeking {
                PlaybackState::Seeking
            } else {
                PlaybackState::Opening
            }
        } else if self.desired_playing {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        }
    }
}

fn activate_unless_video_stalled(shared: &Shared, epoch: u64) {
    let state = shared.state.lock();
    if state.epoch == epoch && state.video_stall_started.is_none() && state.check().is_ok() {
        shared.pcm.activate(epoch);
    }
}

#[cfg(test)]
#[path = "session_output_tests.rs"]
mod tests;
