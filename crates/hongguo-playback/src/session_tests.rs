use super::*;

const SILENT: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-64x48-no-audio.mp4");
const TEST_TIMEOUT: Duration = Duration::from_secs(3);

struct Memory;
impl ReadAt for Memory {
    fn length(&self) -> AppResult<u64> {
        Ok(SILENT.len() as u64)
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
        bytes.extend_from_slice(
            SILENT
                .get(offset as usize..(offset + length) as usize)
                .ok_or_else(|| AppError::Media("fixture read exceeds source".into()))?,
        );
        Ok(())
    }
}

fn frame(session: &PlaybackSession, position: f64) -> Arc<PresentationFrame> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        if let Some(frame) = session.frame_at(position).unwrap() {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "no frame before deadline; state={:?}",
            session.state()
        );
        let mut state = session.shared.state.lock();
        session
            .shared
            .changed
            .wait_for(&mut state, Duration::from_millis(10));
    }
}

#[test]
fn software_session_presents_silent_video_and_seeks_without_audio_devices() {
    let session = PlaybackSession::open(Arc::new(Memory), SessionConfig::default()).unwrap();
    let first = frame(&session, 0.0);
    assert_eq!(
        (
            first.image.descriptor().width,
            first.image.descriptor().height
        ),
        (64, 48)
    );
    assert!(session.holding());
    assert!(session.presented(first.generation, first.serial).unwrap());
    assert!(!session.holding());
    assert_eq!(session.position().unwrap().position_secs, 0.0);
    let epoch = session.seek(0.2).unwrap();
    assert!(!session.presented(first.generation, first.serial).unwrap());
    let sought = frame(&session, 0.2);
    assert_eq!(sought.generation, epoch);
    assert!(sought.pts_secs <= 0.2 && sought.pts_secs + sought.duration_secs >= 0.2);
    assert!(session.info().unwrap().is_some_and(|info| !info.has_audio));
    session.close();
    assert_eq!(session.state(), PlaybackState::Closed);
    assert!(session.frame_at(0.2).is_err());
}

#[test]
fn recovery_replaces_generations_and_enforces_a_session_wide_attempt_limit() {
    let session = PlaybackSession::open(Arc::new(Memory), SessionConfig::default()).unwrap();
    let first = frame(&session, 0.0);
    let mut previous = first.generation;
    for backend in [Backend::VideoToolbox, Backend::D3d11 { device: 0 }] {
        let job = session.shared.job(0).unwrap();
        video::recover(
            &session.shared,
            &job,
            backend,
            "injected backend failure".into(),
        );
        let state = session.shared.state.lock();
        assert_eq!(state.mode, PlaybackState::Recovering);
        assert!(state.epoch > previous);
        assert!(job.cancel.load(Ordering::Acquire));
        assert!(
            state
                .failed
                .iter()
                .any(|key| key.0 == backend && key.1 == 0)
        );
        previous = state.epoch;
    }
    let job = session.shared.job(0).unwrap();
    video::recover(
        &session.shared,
        &job,
        Backend::VideoToolbox,
        "third failure".into(),
    );
    assert_eq!(session.state(), PlaybackState::Failed);
    assert!(session.position().is_err());
}

#[test]
fn deferred_binding_indexes_without_starting_a_decoder() {
    let session = PlaybackSession::open(
        Arc::new(Memory),
        SessionConfig {
            defer_decoder_binding: true,
            ..Default::default()
        },
    )
    .unwrap();
    let deadline = Instant::now() + TEST_TIMEOUT;
    while session.info().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        let mut state = session.shared.state.lock();
        session
            .shared
            .changed
            .wait_for(&mut state, Duration::from_millis(10));
    }
    assert!(session.frame_at(0.0).unwrap().is_none());
    assert!(!session.shared.state.lock().decoder_bound);
    session.configure_decoder(None).unwrap();
    assert_eq!(frame(&session, 0.0).generation, 1);
    assert_eq!(session.shared.state.lock().recovery_attempts, 0);
}

#[test]
fn presentation_recovery_keeps_decoder_admission_and_shares_the_total_budget() {
    let session = PlaybackSession::open(Arc::new(Memory), SessionConfig::default()).unwrap();
    let first = frame(&session, 0.0);
    assert!(session.presented(first.generation, first.serial).unwrap());
    let mut previous = first.generation;
    let failed = session.shared.state.lock().failed.clone();
    for attempt in 1..=2 {
        session
            .recover_presentation("output device replaced".into())
            .unwrap();
        let state = session.shared.state.lock();
        assert!(state.device.is_none());
        assert_eq!(state.failed, failed);
        assert!(state.epoch > previous);
        assert_eq!(state.recovery_attempts, attempt);
        assert_eq!(state.mode, PlaybackState::Recovering);
        assert!(state.holding);
        previous = state.epoch;
    }
    assert!(
        session
            .recover_presentation("third output failure".into())
            .is_err()
    );
    assert_eq!(session.state(), PlaybackState::Failed);
}

struct ToolboxDevice(u64);
unsafe impl DecodeDevice for ToolboxDevice {
    fn kind(&self) -> HardwareKind {
        HardwareKind::VideoToolbox
    }
    fn device(&self) -> usize {
        0
    }
    fn generation(&self) -> u64 {
        self.0
    }
}

#[test]
fn repeated_device_rebuilds_cannot_reset_the_session_recovery_budget() {
    let _permit = crate::budget::DecoderPermit::acquire(u32::MAX, &AtomicBool::new(false)).unwrap();
    let session = PlaybackSession::open(
        Arc::new(Memory),
        SessionConfig {
            defer_decoder_binding: true,
            ..Default::default()
        },
    )
    .unwrap();
    session.configure_decoder(None).unwrap();
    session
        .configure_decoder(Some(Arc::new(ToolboxDevice(1))))
        .unwrap();
    assert_eq!(session.shared.state.lock().recovery_attempts, 1);
    session.configure_decoder(None).unwrap();
    assert_eq!(session.shared.state.lock().recovery_attempts, 2);
    assert!(
        session
            .configure_decoder(Some(Arc::new(ToolboxDevice(2))))
            .is_err()
    );
    assert_eq!(session.state(), PlaybackState::Failed);
}
