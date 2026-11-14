use super::*;

const VIDEO: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-64x48-no-audio.mp4");

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
        bytes.extend_from_slice(
            VIDEO
                .get(offset as usize..(offset + length) as usize)
                .ok_or_else(|| AppError::Media("fixture read exceeds source".into()))?,
        );
        Ok(())
    }
}

struct Device(HardwareKind);
unsafe impl DecodeDevice for Device {
    fn kind(&self) -> HardwareKind {
        self.0
    }
    fn device(&self) -> usize {
        1
    }
    fn generation(&self) -> u64 {
        7
    }
}

fn indexed_session(kind: Option<HardwareKind>) -> (PlaybackSession, Job) {
    let session = PlaybackSession::open(
        Arc::new(Memory),
        SessionConfig {
            defer_decoder_binding: true,
            ..Default::default()
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut state = session.shared.state.lock();
    while state.index.is_none() {
        state.check().unwrap();
        assert!(Instant::now() < deadline, "fixture index was not ready");
        session
            .shared
            .changed
            .wait_for(&mut state, Duration::from_millis(10));
    }
    state.device = kind.map(|kind| Arc::new(Device(kind)) as Arc<dyn DecodeDevice>);
    let job = Job {
        epoch: state.epoch,
        target_secs: state.target_secs,
        rate: state.rate,
        cancel: state.cancel.clone(),
        index: state.index.clone().unwrap(),
        device: state.device.clone(),
        stream_key: state.stream_key.unwrap(),
    };
    drop(state);
    (session, job)
}

fn invalid_data() -> PipelineError {
    CodecError::Native {
        operation: "receive video",
        code: -1_094_995_529,
        description: "invalid data found when processing input".into(),
    }
    .into()
}

#[test]
fn hardware_invalid_data_verifies_the_same_source_in_software_within_the_budget() {
    for kind in [
        HardwareKind::VideoToolbox,
        HardwareKind::D3d11,
        HardwareKind::MediaCodec,
    ] {
        let (session, job) = indexed_session(Some(kind));
        let source = session.shared.source.clone();
        let backend = select_backend(&session.shared, &job);
        assert_ne!(backend, Backend::Software);
        handle_failure(&session.shared, &job, backend, invalid_data());
        let state = session.shared.state.lock();
        assert_eq!(state.mode, PlaybackState::Recovering, "{kind:?}");
        assert_eq!(state.recovery_attempts, 1);
        assert!(state.holding);
        assert!(state.epoch > job.epoch);
        assert!(state.error.is_none());
        assert!(job.cancel.load(Ordering::Acquire));
        assert!(Arc::ptr_eq(&source, &session.shared.source));
        drop(state);
        assert_eq!(select_backend(&session.shared, &job), Backend::Software);
    }
}

#[test]
fn software_invalid_data_is_terminal_without_repeated_decoder_recovery() {
    let (session, job) = indexed_session(None);
    handle_failure(&session.shared, &job, Backend::Software, invalid_data());
    let state = session.shared.state.lock();
    assert_eq!(state.mode, PlaybackState::Failed);
    assert_eq!(state.recovery_attempts, 0);
    assert!(state.failed.is_empty());
    assert!(state.cancel.load(Ordering::Acquire));
}

#[test]
fn source_read_failures_do_not_blacklist_the_hardware_decoder() {
    let (session, job) = indexed_session(Some(HardwareKind::D3d11));
    let backend = select_backend(&session.shared, &job);
    handle_failure(
        &session.shared,
        &job,
        backend,
        AppError::Io("source read failed".into()).into(),
    );
    let state = session.shared.state.lock();
    assert_eq!(state.mode, PlaybackState::Failed);
    assert_eq!(state.recovery_attempts, 0);
    assert!(state.failed.is_empty());
}
