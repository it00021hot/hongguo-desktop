use super::tests::{MemorySource, SAMPLE, frame};
use super::*;
use hongguo_media_types::{
    DecodedVideoFrame, FrameImage, HardwareKind, HevcDecoderConfig, VideoDecoder,
};
use std::sync::atomic::AtomicU32;

struct Device {
    opens: Arc<AtomicU32>,
    failure: FailureStage,
    generation: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FailureStage {
    None,
    Initialize,
    Decode,
}
struct Hardware {
    software: hongguo_hevc::Decoder,
    fails: bool,
}
// SAFETY: this fake owns no raw handles; its decoder is confined to the session worker.
unsafe impl DecodeDevice for Device {
    fn kind(&self) -> HardwareKind {
        HardwareKind::D3d11
    }
    fn device(&self) -> usize {
        0
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    fn hevc_decoder(
        &self,
        config: &HevcDecoderConfig,
    ) -> Result<Option<Box<dyn VideoDecoder>>, String> {
        self.opens.fetch_add(1, Ordering::AcqRel);
        if self.failure == FailureStage::Initialize {
            return Err("injected unsupported HEVC GPU profile".into());
        }
        Ok(Some(Box::new(Hardware {
            software: hongguo_hevc::Decoder::new(1, config.timescale).unwrap(),
            fails: self.failure == FailureStage::Decode,
        })))
    }
}
impl VideoDecoder for Hardware {
    fn decode(&mut self, bytes: &[u8], pts: i64) -> Result<Vec<DecodedVideoFrame>, String> {
        if self.fails {
            return Err("injected D3D11 submission failure".into());
        }
        self.software
            .decode(bytes, pts)
            .map(convert)
            .map_err(|error| error.to_string())
    }
    fn drain(&mut self) -> Result<Vec<DecodedVideoFrame>, String> {
        self.software
            .drain()
            .map(convert)
            .map_err(|error| error.to_string())
    }
}
fn convert(frames: Vec<hongguo_media_types::NativeFrame>) -> Vec<DecodedVideoFrame> {
    frames
        .into_iter()
        .map(|frame| DecodedVideoFrame {
            pts_secs: frame.pts.unwrap(),
            image: FrameImage::Cpu(Arc::new(frame)),
        })
        .collect()
}
fn session(failure: FailureStage) -> (PlaybackSession, Arc<AtomicU32>) {
    let session = PlaybackSession::open(
        Arc::new(MemorySource(SAMPLE.to_vec())),
        SessionConfig {
            defer_decoder_binding: true,
            ..Default::default()
        },
    )
    .unwrap();
    let opens = Arc::new(AtomicU32::new(0));
    session
        .configure_decoder(Some(Arc::new(Device {
            opens: opens.clone(),
            failure,
            generation: 1,
        })))
        .unwrap();
    (session, opens)
}

#[test]
fn bound_native_device_is_tried_before_software_and_reused_after_seek() {
    let (session, opens) = session(FailureStage::None);
    frame(&session, 0.0);
    assert_eq!(opens.load(Ordering::Acquire), 1);
    let generation = session.seek(0.8).unwrap();
    assert_eq!(frame(&session, 0.8).generation, generation);
    assert_eq!(opens.load(Ordering::Acquire), 2);
}

#[test]
fn failed_hardware_restarts_a_fresh_epoch_and_stays_software_after_seek() {
    let (session, opens) = session(FailureStage::Decode);
    let first = frame(&session, 0.0);
    assert!(first.generation > 1, "failed native frames must be retired");
    assert_eq!(opens.load(Ordering::Acquire), 1);
    session.seek(0.8).unwrap();
    frame(&session, 0.8);
    assert_eq!(
        opens.load(Ordering::Acquire),
        1,
        "failed hardware was retried"
    );
}

#[test]
fn missing_hardware_profile_uses_software_once_and_keeps_seek_working() {
    let (session, opens) = session(FailureStage::Initialize);
    frame(&session, 0.0);
    assert_eq!(opens.load(Ordering::Acquire), 1);
    let generation = session.seek(0.8).unwrap();
    assert_eq!(frame(&session, 0.8).generation, generation);
    assert_eq!(opens.load(Ordering::Acquire), 1);
}

#[test]
fn rejected_native_surface_restarts_software_at_the_audio_clock_position() {
    let (session, opens) = session(FailureStage::None);
    let clock = super::tests::FakeClock::new(true);
    session.configure_clock(clock.clone()).unwrap();
    session.set_playing(true).unwrap();
    let old = frame(&session, 0.0);
    session.presented(old.generation, old.serial).unwrap();
    clock.position.lock().as_mut().unwrap().seconds = 0.4;
    session
        .reject_decoder("injected P010 texture import failure".into())
        .unwrap();
    assert!(!session.presented(old.generation, old.serial).unwrap());
    let current = frame(&session, 0.4);
    assert!(current.generation > old.generation);
    assert!(current.pts_secs <= 0.4 && current.pts_secs + current.duration_secs > 0.4);
    assert_eq!(session.position().unwrap().position_secs, 0.4);
    session
        .presented(current.generation, current.serial)
        .unwrap();
    assert!(!session.holding());
    assert_eq!(opens.load(Ordering::Acquire), 1);
    assert!(
        session
            .reject_decoder("software presentation failed".into())
            .is_err()
    );
}

#[test]
fn a_rebuilt_device_can_retry_hardware_after_the_previous_device_failed() {
    let (session, old_opens) = session(FailureStage::Decode);
    let old = frame(&session, 0.0);
    assert_eq!(old_opens.load(Ordering::Acquire), 1);
    let opens = Arc::new(AtomicU32::new(0));
    session
        .configure_decoder(Some(Arc::new(Device {
            opens: opens.clone(),
            failure: FailureStage::None,
            generation: 2,
        })))
        .unwrap();
    let current = frame(&session, 0.0);
    assert!(current.generation > old.generation);
    assert_eq!(opens.load(Ordering::Acquire), 1);
}
