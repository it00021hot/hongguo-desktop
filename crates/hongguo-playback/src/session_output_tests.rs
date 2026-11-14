use super::*;
use hongguo_audio_api::{AudioErrorKind, PcmSource, Position};
use std::sync::atomic::AtomicU64;

const VIDEO: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const DEADLINE: Duration = Duration::from_secs(3);

struct Memory;
impl ReadAt for Memory {
    fn length(&self) -> AppResult<u64> {
        Ok(VIDEO.len() as u64)
    }
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        bytes.clear();
        bytes.extend_from_slice(&VIDEO[offset as usize..(offset + length) as usize]);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Open,
    Flush,
    Start,
    Pause,
    Position,
}

#[derive(Default)]
struct Control {
    opens: AtomicU64,
    drops: AtomicU64,
    starts: AtomicU64,
    positions: AtomicU64,
    cursor: AtomicU64,
    late_cursor: AtomicU64,
    fault: Mutex<Option<(Operation, AudioErrorKind, u32)>>,
    reopen_gate: Mutex<Option<Arc<OpenGate>>>,
    gate_first_open: AtomicBool,
}

#[derive(Default)]
struct OpenGate {
    released: Mutex<bool>,
    changed: Condvar,
}
impl OpenGate {
    fn wait(&self) -> Result<(), AudioError> {
        let deadline = Instant::now() + DEADLINE;
        let mut released = self.released.lock();
        while !*released {
            if Instant::now() >= deadline {
                return Err(AudioError::callback(
                    "test reopen gate exceeded its deadline",
                ));
            }
            self.changed
                .wait_for(&mut released, Duration::from_millis(5));
        }
        Ok(())
    }
    fn release(&self) {
        *self.released.lock() = true;
        self.changed.notify_all();
    }
}
struct ReleaseGate(Arc<OpenGate>);
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl Control {
    fn arm(&self, operation: Operation, kind: AudioErrorKind, count: u32) {
        *self.fault.lock() = Some((operation, kind, count));
    }
    fn check(&self, operation: Operation) -> Result<(), AudioError> {
        let mut fault = self.fault.lock();
        let Some((target, kind, remaining)) = fault.as_mut() else {
            return Ok(());
        };
        if *target != operation || *remaining == 0 {
            return Ok(());
        }
        *remaining -= 1;
        Err(match kind {
            AudioErrorKind::Device => AudioError::new("injected device disconnected"),
            AudioErrorKind::InvalidFormat => AudioError::invalid_format("injected invalid PCM"),
            AudioErrorKind::Callback => AudioError::callback("injected source callback failed"),
        })
    }
}

struct Device {
    control: Arc<Control>,
    pcm: Arc<PcmRing>,
    format: PcmFormat,
    epoch: u64,
}
impl AudioOutput for Device {
    fn start(&mut self) -> Result<(), AudioError> {
        self.control.check(Operation::Start)?;
        self.control.starts.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn pause(&mut self) -> Result<(), AudioError> {
        self.control.check(Operation::Pause)
    }
    fn flush(&mut self, epoch: u64) -> Result<(), AudioError> {
        self.control.check(Operation::Flush)?;
        self.epoch = epoch;
        self.control.cursor.store(0, Ordering::Release);
        Ok(())
    }
    fn position(&self) -> Result<Position, AudioError> {
        self.control.positions.fetch_add(1, Ordering::AcqRel);
        self.control.check(Operation::Position)?;
        let frames = self.control.cursor.load(Ordering::Acquire);
        self.pcm.played(self.epoch, frames);
        Ok(Position {
            epoch: self.epoch,
            device_frames: frames,
            latency_frames: 0,
        })
    }
    fn format(&self) -> PcmFormat {
        self.format
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        let late = self.control.late_cursor.swap(0, Ordering::AcqRel);
        if late != 0 {
            self.pcm.synchronize_position(self.epoch, late);
        }
        self.control.drops.fetch_add(1, Ordering::AcqRel);
    }
}

fn open(control: &Arc<Control>) -> PlaybackSession {
    open_config(control, SessionConfig::default())
}

fn open_config(control: &Arc<Control>, mut config: SessionConfig) -> PlaybackSession {
    config.decoder_threads = 1;
    let control = control.clone();
    PlaybackSession::open_with_output(Arc::new(Memory), config, move |shared| {
        run_with(shared, |shared| {
            let attempt = control.opens.fetch_add(1, Ordering::AcqRel) + 1;
            control.check(Operation::Open)?;
            let gate = control.reopen_gate.lock().clone();
            if (attempt > 1 || control.gate_first_open.load(Ordering::Acquire))
                && let Some(gate) = gate
            {
                gate.wait()?;
            }
            Ok(Box::new(Device {
                control: control.clone(),
                pcm: shared.pcm.clone(),
                format: shared.config.pcm_format,
                epoch: 0,
            }))
        });
    })
    .unwrap()
}

fn wait(session: &PlaybackSession, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out; state={:?}",
            session.state()
        );
        let mut state = session.shared.state.lock();
        session
            .shared
            .changed
            .wait_for(&mut state, Duration::from_millis(5));
    }
}

fn present(session: &PlaybackSession, position: f64) -> u64 {
    let mut frame = None;
    wait(session, || {
        frame = session.frame_at(position).unwrap();
        frame.is_some()
    });
    let frame = frame.unwrap();
    assert!(session.presented(frame.generation, frame.serial).unwrap());
    frame.generation
}

#[test]
fn an_idle_device_callback_failure_is_detected_by_the_playing_position_heartbeat() {
    let control = Arc::new(Control::default());
    let session = open(&control);
    session.set_playing(true).unwrap();
    present(&session, 0.);
    wait(&session, || control.starts.load(Ordering::Acquire) == 1);
    control.arm(Operation::Position, AudioErrorKind::Device, 1);
    wait(&session, || control.opens.load(Ordering::Acquire) == 2);
    let state = session.shared.state.lock();
    assert_eq!(state.mode, PlaybackState::Recovering);
    assert_eq!(state.recovery_attempts, 1);
    assert!(state.holding);
    assert!(state.failed.is_empty());
    assert_eq!(control.starts.load(Ordering::Acquire), 1);
}

#[test]
fn a_paused_output_sleeps_and_rechecks_the_device_before_any_resume_pcm() {
    let control = Arc::new(Control::default());
    let session = open(&control);
    session.set_playing(true).unwrap();
    present(&session, 0.);
    wait(&session, || control.starts.load(Ordering::Acquire) == 1);
    session.set_playing(false).unwrap();
    wait(&session, || control.positions.load(Ordering::Acquire) >= 2);
    let positions = control.positions.load(Ordering::Acquire);
    std::thread::sleep(HEARTBEAT * 2 + Duration::from_millis(50));
    assert_eq!(control.positions.load(Ordering::Acquire), positions);
    control.arm(Operation::Position, AudioErrorKind::Device, 1);
    session.set_playing(true).unwrap();
    wait(&session, || control.opens.load(Ordering::Acquire) == 2);
    assert_eq!(
        control.starts.load(Ordering::Acquire),
        1,
        "old route must never restart"
    );
    assert!(session.holding());
}

#[test]
fn replacement_anchors_after_old_device_retirement_and_ignores_its_late_epoch() {
    let control = Arc::new(Control::default());
    let session = open(&control);
    let source = session.shared.source.clone();
    session.set_playing(true).unwrap();
    present(&session, 0.);
    wait(&session, || control.starts.load(Ordering::Acquire) == 1);
    wait(&session, || session.pcm().queued_frames() >= 4_800);
    let pcm = session.pcm();
    assert_eq!(pcm.read(&mut vec![0.; 4_800 * 2]).media_frames, 4_800);
    control.cursor.store(2_400, Ordering::Release);
    wait(&session, || pcm.position() >= 0.049);
    control.late_cursor.store(4_800, Ordering::Release);
    control.arm(Operation::Position, AudioErrorKind::Device, 1);
    wait(&session, || control.opens.load(Ordering::Acquire) == 2);
    let state = session.shared.state.lock();
    let anchor = state.target_secs;
    assert!(
        anchor > 0.09 && anchor < 0.11,
        "last real consumption={anchor}"
    );
    assert_eq!(state.epoch, 2);
    assert_eq!(state.recovery_attempts, 1);
    assert!(state.failed.is_empty());
    assert!(Arc::ptr_eq(&source, &session.shared.source));
    drop(state);
    assert_eq!(control.drops.load(Ordering::Acquire), 1);
    assert_eq!(control.starts.load(Ordering::Acquire), 1);
    pcm.played(1, 100_000);
    assert_eq!(pcm.position(), anchor);
    session.set_playing(false).unwrap();
    assert_eq!(present(&session, anchor), 2);
    wait(&session, || session.state() == PlaybackState::Paused);
    assert_eq!(control.starts.load(Ordering::Acquire), 1);
    session.set_playing(true).unwrap();
    wait(&session, || control.starts.load(Ordering::Acquire) == 2);
}

#[test]
fn open_flush_start_and_pause_device_errors_share_the_bounded_recovery_path() {
    for operation in [
        Operation::Open,
        Operation::Flush,
        Operation::Start,
        Operation::Pause,
    ] {
        let control = Arc::new(Control::default());
        if operation != Operation::Pause {
            control.arm(operation, AudioErrorKind::Device, 1);
        }
        let session = open(&control);
        if matches!(operation, Operation::Start | Operation::Pause) {
            session.set_playing(true).unwrap();
            present(&session, 0.);
        }
        if operation == Operation::Pause {
            wait(&session, || control.starts.load(Ordering::Acquire) == 1);
            control.arm(Operation::Pause, AudioErrorKind::Device, 1);
            session.set_playing(false).unwrap();
        }
        wait(&session, || control.opens.load(Ordering::Acquire) == 2);
        let state = session.shared.state.lock();
        assert_eq!(state.mode, PlaybackState::Recovering, "{operation:?}");
        assert_eq!(state.recovery_attempts, 1);
        assert!(state.holding);
        assert!(state.failed.is_empty());
    }
}

#[test]
fn callback_and_format_contract_errors_are_terminal_without_device_retries() {
    for kind in [AudioErrorKind::Callback, AudioErrorKind::InvalidFormat] {
        let control = Arc::new(Control::default());
        let session = open(&control);
        session.set_playing(true).unwrap();
        present(&session, 0.);
        wait(&session, || control.starts.load(Ordering::Acquire) == 1);
        control.arm(Operation::Position, kind, 1);
        wait(&session, || session.state() == PlaybackState::Failed);
        let state = session.shared.state.lock();
        assert_eq!(state.recovery_attempts, 0);
        assert!(state.failed.is_empty());
        assert_eq!(control.opens.load(Ordering::Acquire), 1);
    }
}

#[test]
fn reopen_failures_exhaust_the_session_attempt_and_elapsed_budgets() {
    for (attempts, elapsed, expected_opens, expected_attempts) in [
        (2, Duration::from_secs(15), 3, 2),
        (10, Duration::from_millis(30), 1, 1),
    ] {
        let control = Arc::new(Control::default());
        control.arm(Operation::Open, AudioErrorKind::Device, u32::MAX);
        let session = open_config(
            &control,
            SessionConfig {
                recovery: RecoveryLimits { attempts, elapsed },
                ..Default::default()
            },
        );
        wait(&session, || session.state() == PlaybackState::Failed);
        let state = session.shared.state.lock();
        assert_eq!(state.recovery_attempts, expected_attempts);
        assert!(state.failed.is_empty());
        assert_eq!(control.opens.load(Ordering::Acquire), expected_opens);
        assert!(state.error.as_ref().unwrap().contains("budget exhausted"));
    }
}

#[test]
fn a_visible_video_frame_does_not_complete_recovery_before_the_audio_device_is_ready() {
    let control = Arc::new(Control::default());
    let gate = Arc::new(OpenGate::default());
    let _release = ReleaseGate(gate.clone());
    *control.reopen_gate.lock() = Some(gate.clone());
    control.arm(Operation::Open, AudioErrorKind::Device, 1);
    let session = open(&control);
    session.set_playing(true).unwrap();
    wait(&session, || control.opens.load(Ordering::Acquire) == 2);
    assert_eq!(present(&session, 0.), 2);
    assert!(session.position().unwrap().buffering);
    assert_eq!(session.state(), PlaybackState::Recovering);
    assert!(session.shared.state.lock().recovery_started.is_some());
    assert_eq!(control.starts.load(Ordering::Acquire), 0);
    gate.release();
    wait(&session, || control.starts.load(Ordering::Acquire) == 1);
    wait(&session, || !session.position().unwrap().buffering);
    assert!(session.shared.state.lock().recovery_started.is_none());
}

#[test]
fn the_initial_audio_open_keeps_a_fast_first_video_frame_buffering_without_recovery() {
    let control = Arc::new(Control::default());
    let gate = Arc::new(OpenGate::default());
    let _release = ReleaseGate(gate.clone());
    *control.reopen_gate.lock() = Some(gate.clone());
    control.gate_first_open.store(true, Ordering::Release);
    let session = open(&control);
    session.set_playing(true).unwrap();
    wait(&session, || control.opens.load(Ordering::Acquire) == 1);
    assert_eq!(present(&session, 0.), 1);
    assert!(session.position().unwrap().buffering);
    assert_eq!(session.state(), PlaybackState::Opening);
    assert_eq!(session.shared.state.lock().recovery_attempts, 0);
    assert!(session.shared.state.lock().recovery_started.is_none());
    assert_eq!(control.starts.load(Ordering::Acquire), 0);
    gate.release();
    wait(&session, || control.starts.load(Ordering::Acquire) == 1);
    wait(&session, || !session.position().unwrap().buffering);
    assert_eq!(session.state(), PlaybackState::Playing);
    assert_eq!(session.shared.state.lock().recovery_attempts, 0);
}
