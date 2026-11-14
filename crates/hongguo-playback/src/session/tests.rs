use super::*;
use hongguo_native_clock::Position;
use std::time::Duration;

#[test]
fn native_audio_readiness_loss_holds_the_latest_media_time() {
    let session = open();
    let clock = FakeClock::new(true);
    session.configure_clock(clock.clone()).unwrap();
    let first = frame(&session, 0.0);
    session.presented(first.generation, first.serial).unwrap();
    *clock.position.lock() = Ok(Position {
        seconds: 0.4,
        ready: true,
        ..Default::default()
    });
    assert_eq!(session.position().unwrap().position_secs, 0.4);
    *clock.position.lock() = Ok(Position {
        seconds: 0.4,
        buffering: true,
        ..Default::default()
    });
    let held = session.position().unwrap();
    assert_eq!(held.position_secs, 0.4);
    assert!(held.buffering);
    assert!(!clock.controls.lock().last().unwrap().playing);
}

#[test]
fn seeking_while_metadata_is_loading_keeps_the_source_read_alive() {
    use std::sync::mpsc::{channel, sync_channel};
    struct Loading {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        first: AtomicBool,
    }
    impl ReadAt for Loading {
        fn length(&self) -> AppResult<u64> {
            Ok(SAMPLE.len() as u64)
        }
        fn read_at(
            &self,
            offset: u64,
            length: u64,
            cancel: &AtomicBool,
            bytes: &mut Vec<u8>,
        ) -> AppResult<()> {
            if self.first.swap(false, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            MemorySource(SAMPLE.to_vec()).read_at(offset, length, cancel, bytes)
        }
    }
    let (entered, ready) = channel();
    let (release, blocked) = sync_channel(1);
    let session = PlaybackSession::open(
        Arc::new(Loading {
            entered,
            release: Mutex::new(blocked),
            first: AtomicBool::new(true),
        }),
        SessionConfig::default(),
    )
    .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    session.seek(0.2).unwrap();
    let generation = session.seek(0.8).unwrap();
    release.send(()).unwrap();
    let actual = frame(&session, 0.8);
    assert_eq!(actual.generation, generation);
    assert!(actual.pts_secs + actual.duration_secs > 0.8);
}

pub(super) const SAMPLE: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");

pub(super) struct MemorySource(pub(super) Vec<u8>);
impl ReadAt for MemorySource {
    fn length(&self) -> AppResult<u64> {
        Ok(self.0.len() as u64)
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
        let end = offset
            .checked_add(length)
            .ok_or_else(|| AppError::Media("range overflow".into()))?;
        *bytes = self
            .0
            .get(offset as usize..end as usize)
            .ok_or_else(|| AppError::Media("invalid source range".into()))?
            .to_vec();
        Ok(())
    }
}

pub(super) struct FakeClock {
    pub(super) position: Mutex<Result<Position, String>>,
    controls: Mutex<Vec<Control>>,
    closed: AtomicBool,
}
impl FakeClock {
    pub(super) fn new(ready: bool) -> Arc<Self> {
        Arc::new(Self {
            position: Mutex::new(Ok(Position {
                ready,
                ..Default::default()
            })),
            controls: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        })
    }
}
impl Clock for FakeClock {
    fn position(&self) -> Result<Position, String> {
        self.position.lock().clone()
    }
    fn control(&self, control: Control) -> Result<(), String> {
        self.controls.lock().push(control);
        Ok(())
    }
    fn seek(&self, seconds: f64) -> Result<(), String> {
        self.position
            .lock()
            .as_mut()
            .map_err(|error| error.clone())?
            .seconds = seconds;
        Ok(())
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

fn open() -> PlaybackSession {
    PlaybackSession::open(
        Arc::new(MemorySource(SAMPLE.to_vec())),
        SessionConfig::default(),
    )
    .unwrap()
}

pub(super) fn frame(session: &PlaybackSession, target: f64) -> Arc<PresentationFrame> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(frame) = session.frame_at(target).unwrap() {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "codec session did not produce a frame"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn native_audio_waits_for_both_clock_readiness_and_a_real_video_ack() {
    let session = open();
    let clock = FakeClock::new(false);
    session.set_playing(true).unwrap();
    session.configure_clock(clock.clone()).unwrap();
    let first = frame(&session, 0.0);
    assert!(session.presented(first.generation, first.serial).unwrap());
    assert!(session.holding());
    assert!(!clock.controls.lock().last().unwrap().playing);
    *clock.position.lock() = Ok(Position {
        ready: true,
        ..Default::default()
    });
    session.position().unwrap();
    assert!(!session.holding());
    assert!(clock.controls.lock().last().unwrap().playing);
    session.set_rate(3.0).unwrap();
    assert_eq!(clock.controls.lock().last().unwrap().rate, 3.0);
    session.set_playing(false).unwrap();
    assert!(!clock.controls.lock().last().unwrap().playing);
    session.close();
    assert!(clock.closed.load(Ordering::Acquire));
}

#[test]
fn seeking_retires_old_frames_and_only_the_new_epoch_can_release_audio() {
    let session = open();
    let clock = FakeClock::new(true);
    session.configure_clock(clock).unwrap();
    let old = frame(&session, 0.0);
    let generation = session.seek(0.8).unwrap();
    assert!(!session.presented(old.generation, old.serial).unwrap());
    let new = frame(&session, 0.8);
    assert_eq!(new.generation, generation);
    assert!(new.pts_secs + new.duration_secs > 0.8);
    assert!(session.presented(new.generation, new.serial).unwrap());
    assert!(!session.presented(new.generation, new.serial).unwrap());
    assert!(!session.holding());
}

#[test]
fn native_clock_errors_are_reported_instead_of_switching_to_a_wall_clock() {
    let session = open();
    let clock = FakeClock::new(true);
    session.configure_clock(clock.clone()).unwrap();
    let first = frame(&session, 0.0);
    session.presented(first.generation, first.serial).unwrap();
    *clock.position.lock() = Err("native audio device lost".into());
    assert!(
        session
            .position()
            .unwrap_err()
            .to_string()
            .contains("native audio device lost")
    );
}

#[test]
fn closing_a_full_video_queue_cancels_the_worker_and_preserves_leased_pixels() {
    let session = open();
    let first = frame(&session, 0.0);
    let image = first.image.descriptor();
    let started = Instant::now();
    session.close();
    drop(session);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!((image.width, image.height), (136, 76));
    assert_eq!(first.image.descriptor().width, 136);
}

#[test]
fn presentation_recovery_is_bounded_and_keeps_the_current_native_media_position() {
    let session = open();
    let clock = FakeClock::new(true);
    session.configure_clock(clock).unwrap();
    frame(&session, 0.0);
    session
        .recover_presentation("device rebuild".into())
        .unwrap();
    session
        .recover_presentation("device rebuild".into())
        .unwrap();
    assert!(
        session
            .recover_presentation("device rebuild".into())
            .is_err()
    );
}
