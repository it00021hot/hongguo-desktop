use super::*;
use hongguo_audio_api::PcmSource;
use hongguo_media_types::{ChromaSampling, ColorMetadata, CropRect, NativeFrame};

const VIDEO: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-64x48-no-audio.mp4");
const AUDIO_VIDEO: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");

struct Memory(&'static [u8]);
impl ReadAt for Memory {
    fn length(&self) -> AppResult<u64> {
        Ok(self.0.len() as u64)
    }
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        _cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        bytes.clear();
        bytes.extend_from_slice(&self.0[offset as usize..(offset + length) as usize]);
        Ok(())
    }
}

fn session(bytes: &'static [u8]) -> PlaybackSession {
    let session = PlaybackSession::open(
        Arc::new(Memory(bytes)),
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
        assert!(Instant::now() < deadline);
        session
            .shared
            .changed
            .wait_for(&mut state, Duration::from_millis(10));
    }
    state.current = Some(frame(1, 0., 0.1));
    state.holding = false;
    state.desired_playing = true;
    state.mode = PlaybackState::Playing;
    state.audio_output_pending = false;
    state.silent_clock.seek(0.2, Instant::now()).unwrap();
    state.silent_clock.set_running(true, Instant::now());
    drop(state);
    session
}

fn frame(serial: u64, pts_secs: f64, duration_secs: f64) -> Arc<PresentationFrame> {
    Arc::new(PresentationFrame {
        serial,
        generation: 1,
        pts_secs,
        duration_secs,
        image: FrameImage::Cpu(Arc::new(NativeFrame {
            width: 1,
            height: 1,
            bit_depth: 8,
            chroma: ChromaSampling::Monochrome,
            planes: Vec::new(),
            pts: Some(pts_secs),
            color: ColorMetadata::default(),
            crop: CropRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            rotation_degrees: 0,
        })),
    })
}

fn supply(session: &PlaybackSession, serial: u64, start: f64, duration: f64) {
    session.shared.state.lock().queue.push_back(QueuedFrame {
        frame: frame(serial, start, duration),
        bytes: 0,
    });
}

#[test]
fn video_starvation_freezes_the_current_clock_until_a_covering_frame_is_presented() {
    let session = session(VIDEO);
    let position = session.position().unwrap();
    assert!(position.buffering);
    assert!(position.position_secs >= 0.2);
    assert!(session.holding());
    assert_eq!(session.state(), PlaybackState::Buffering);
    assert_eq!(
        session.position().unwrap().position_secs,
        position.position_secs
    );
    assert!(
        !session.presented(1, 1).unwrap(),
        "expired frame cannot release the clock"
    );
    supply(&session, 2, 0.2, 0.1);
    assert_eq!(
        session
            .frame_at(position.position_secs)
            .unwrap()
            .unwrap()
            .serial,
        2
    );
    assert!(session.presented(1, 2).unwrap());
    assert!(!session.holding());
    let state = session.shared.state.lock();
    assert_eq!(state.epoch, 1);
    assert_eq!(state.recovery_attempts, 0);
    assert!(state.failed.is_empty());
}

#[test]
fn video_starvation_preserves_actual_pcm_progress_during_asynchronous_pause() {
    let session = session(AUDIO_VIDEO);
    let pcm = session.pcm();
    pcm.activate(1);
    pcm.push(
        1,
        crate::PcmChunk {
            samples: vec![0.5; 14_400 * 2],
            media_start_secs: 0.,
            media_duration_secs: 0.3,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(pcm.read(&mut vec![0.; 14_400 * 2]).media_frames, 14_400);
    pcm.played(1, 9_600);
    assert!(session.position().unwrap().buffering);
    assert_eq!(pcm.read(&mut [0.; 16]).media_frames, 0);
    // Already submitted audio can finish before the native pause command takes effect.
    pcm.played(1, 12_000);
    assert_eq!(session.position().unwrap().position_secs, 0.25);
    assert!(!session.presented(1, 1).unwrap());
    supply(&session, 2, 0.2, 0.02);
    assert!(
        session.frame_at(0.25).unwrap().is_none(),
        "expired catch-up frames must not reach color/enhancement"
    );
    assert!(
        !session.presented(1, 2).unwrap(),
        "pause latency advanced past this frame"
    );
    supply(&session, 3, 0.24, 0.1);
    assert_eq!(session.frame_at(0.25).unwrap().unwrap().serial, 3);
    assert!(session.presented(1, 3).unwrap());
    pcm.push(
        1,
        crate::PcmChunk {
            samples: vec![0.5; 4_800 * 2],
            media_start_secs: 0.3,
            media_duration_secs: 0.1,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(pcm.read(&mut [0.; 16]).media_frames, 8);
    assert!(session.position().unwrap().position_secs >= 0.25);
}

#[test]
fn a_ready_due_frame_and_a_video_tail_do_not_trigger_starvation() {
    let session = session(VIDEO);
    supply(&session, 2, 0.2, 0.1);
    assert!(!session.position().unwrap().buffering);
    assert_eq!(session.frame_at(0.2).unwrap().unwrap().serial, 2);
    let mut state = session.shared.state.lock();
    state.video_done = true;
    state.silent_clock.seek(5., Instant::now()).unwrap();
    state.silent_clock.set_running(false, Instant::now());
    drop(state);
    assert!(!session.position().unwrap().buffering);
    assert_eq!(
        session.shared.state.lock().current.as_ref().unwrap().serial,
        2
    );
}

fn install_empty_edits(session: &PlaybackSession) {
    use hongguo_video::timing::{EditEntry, MovieTimeline};
    let mut state = session.shared.state.lock();
    let index = Arc::get_mut(state.index.as_mut().unwrap()).unwrap();
    index.video.info.edit_list = [
        (100, -1, 65_536),
        (200, 0, 65_536),
        (100, -1, 65_536),
        (200, 0, 0),
        (200, 0, 65_536),
    ]
    .map(|(segment_duration, media_time, media_rate)| EditEntry {
        segment_duration,
        media_time,
        media_rate,
    })
    .to_vec();
    let timeline =
        MovieTimeline::from_tracks(1000, std::slice::from_ref(&index.video.info)).unwrap();
    index.video.timeline = timeline.track(index.video.info.track_id).unwrap().clone();
    index.duration_secs = 0.8;
}

#[test]
fn empty_edits_dwell_and_repeated_edits_use_their_movie_presentation_intervals() {
    let session = session(VIDEO);
    install_empty_edits(&session);
    for (position, start, duration, gap) in [
        (0.05, 0., 0.01, true),
        (0.35, 0.1, 0.2, true),
        (0.5, 0.4, 0.2, false),
        (0.7, 0.6, 0.2, false),
        (0.85, 0.6, 0.2, false),
    ] {
        let mut state = session.shared.state.lock();
        state.current = Some(frame(2, start, duration));
        state.silent_clock.seek(position, Instant::now()).unwrap();
        state.silent_clock.set_running(false, Instant::now());
        drop(state);
        let actual = session.position().unwrap();
        assert!(!actual.buffering, "position={position}");
        assert_eq!(actual.video_gap, gap, "position={position}");
        if gap {
            assert!(session.shared.state.lock().current.is_none());
            assert!(session.frame_at(position).unwrap().is_none());
        }
    }
    // The same source sample in an earlier edit is not a frame for the repeated edit.
    let mut state = session.shared.state.lock();
    state.current = Some(frame(3, 0.1, 0.2));
    state.silent_clock.seek(0.7, Instant::now()).unwrap();
    drop(state);
    assert!(session.position().unwrap().buffering);
    supply(&session, 4, 0.6, 0.2);
    assert_eq!(session.frame_at(0.7).unwrap().unwrap().serial, 4);
    assert!(session.presented(1, 4).unwrap());
}

#[test]
fn seeking_into_an_empty_edit_does_not_wait_for_nonexistent_presentation() {
    let session = session(VIDEO);
    install_empty_edits(&session);
    session.seek(0.05).unwrap();
    let position = session.position().unwrap();
    assert!(position.video_gap);
    assert!(!position.buffering);
    assert!(!session.holding());
    assert_eq!(session.state(), PlaybackState::Playing);
    assert!(position.position_secs >= 0.05 && position.position_secs < 0.1);
    assert!(session.shared.state.lock().current.is_none());
    assert!(session.frame_at(position.position_secs).unwrap().is_none());
    session.set_playing(false).unwrap();
    assert_eq!(session.state(), PlaybackState::Paused);
    session.seek(0.8).unwrap();
    assert!(
        !session.position().unwrap().video_gap,
        "natural tail is not an empty edit"
    );
}

#[test]
fn empty_edits_drop_expired_queued_media_but_keep_the_next_edit_buffered() {
    let session = session(VIDEO);
    install_empty_edits(&session);
    supply(&session, 10, 0.1, 0.2);
    supply(&session, 11, 0.4, 0.2);
    let mut state = session.shared.state.lock();
    state.silent_clock.seek(0.35, Instant::now()).unwrap();
    state.silent_clock.set_running(false, Instant::now());
    drop(state);
    assert!(session.position().unwrap().video_gap);
    let mut state = session.shared.state.lock();
    assert!(state.current.is_none());
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.front().unwrap().frame.serial, 11);
    state.silent_clock.seek(0.45, Instant::now()).unwrap();
    drop(state);
    let position = session.position().unwrap();
    assert!(!position.video_gap);
    assert!(!position.buffering);
    assert_eq!(
        session
            .frame_at(position.position_secs)
            .unwrap()
            .unwrap()
            .serial,
        11
    );
}
