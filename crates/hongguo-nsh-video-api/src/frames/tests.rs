use super::*;
use std::sync::atomic::AtomicU64;

#[derive(Default)]
struct Fake {
    closed: AtomicBool,
    generation: AtomicU64,
    controls: Mutex<Vec<PlaybackControl>>,
}
impl FrameSource for Fake {
    fn control(&self, intent: PlaybackControl) -> Result<(), String> {
        self.controls.lock().unwrap().push(intent);
        Ok(())
    }
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        Ok(Some(FrameInfo {
            width: 2,
            height: 2,
        }))
    }
    fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        Ok(Some(Arc::new(PixelFrame {
            serial: 1,
            generation: self.generation.load(Ordering::Acquire),
            pts_ms: position_ms,
            width: 2,
            height: 2,
            stride: 8,
            pixels: vec![0; 16].into(),
        })))
    }
    fn seek(&self, _: u64) -> Result<u64, String> {
        Ok(self.generation.fetch_add(1, Ordering::AcqRel) + 1)
    }
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        Ok(generation == self.generation.load(Ordering::Acquire) && serial == 1)
    }
    fn holding(&self) -> bool {
        true
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

#[test]
fn retiring_a_consumer_cannot_pause_or_reconfigure_its_replacement() {
    let provider = Arc::new(Fake::default());
    let registration = register_frames(&token(), provider.clone()).unwrap();
    let old = open_frames(registration.url()).unwrap();
    let replacement = open_frames(registration.url()).unwrap();
    let initial = PlaybackControl {
        playing: true,
        rate: 1.0,
        volume: 1.0,
        muted: false,
    };
    let latest = PlaybackControl {
        rate: 3.0,
        volume: 0.25,
        muted: true,
        ..initial
    };
    old.control(initial).unwrap();
    replacement.control(latest).unwrap();
    assert!(!old.pause_if_owner().unwrap());
    assert!(!old.control_if_owner(initial).unwrap());
    assert_eq!(old.seek_if_owner(42).unwrap(), None);
    assert!(!old.presented_if_owner(0, 1).unwrap());
    assert_eq!(provider.generation.load(Ordering::Acquire), 0);
    assert_eq!(replacement.seek_if_owner(42).unwrap(), Some(1));
    assert_eq!(
        provider.controls.lock().unwrap().as_slice(),
        &[initial, latest]
    );
    assert!(replacement.pause_if_owner().unwrap());
    assert_eq!(
        *provider.controls.lock().unwrap().last().unwrap(),
        PlaybackControl {
            playing: false,
            ..latest
        }
    );
}
fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("frame-test-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn consumers_close_independently_and_registration_revokes_old_generations() {
    let token = token();
    let source = Arc::new(Fake::default());
    let registration = register_frames(&token, source.clone()).unwrap();
    let url = registration.url().to_owned();
    let first = open_frames(&url).unwrap();
    let second = open_frames(&url).unwrap();
    assert_eq!(
        first.info().unwrap(),
        Some(FrameInfo {
            width: 2,
            height: 2
        })
    );
    assert_eq!(first.seek(42).unwrap(), 1);
    assert_eq!(second.frame_at(42).unwrap().unwrap().pts_ms, 42);
    assert!(first.presented(1, 1).unwrap());
    first.close();
    assert!(first.info().is_err());
    assert!(first.holding().is_err());
    assert!(!source.closed.load(Ordering::Acquire));
    assert!(second.holding().unwrap());
    drop(registration);
    assert!(source.closed.load(Ordering::Acquire));
    assert!(second.seek(10).is_err());
    assert!(second.frame_at(10).is_err());
    assert!(second.presented(1, 1).is_err());
    assert!(second.holding().is_err());
    assert!(open_frames(&url).is_err());
    let replacement = register_frames(&token, Arc::new(Fake::default())).unwrap();
    assert!(open_frames(replacement.url()).unwrap().info().is_ok());
    assert!(second.info().is_err());
}

#[test]
fn only_registered_opaque_tokens_are_accepted() {
    for invalid in ["", "a/b", "a?query", "a#fragment", "https://example.test"] {
        assert!(register_frames(invalid, Arc::new(Fake::default())).is_err());
    }
    for invalid in [
        "https://example.test",
        "hongguo-frames://missing",
        "hongguo-frames://a/b",
    ] {
        assert!(open_frames(invalid).is_err());
    }
    let token = token();
    let _registration = register_frames(&token, Arc::new(Fake::default())).unwrap();
    assert!(register_frames(&token, Arc::new(Fake::default())).is_err());
}

#[test]
fn frame_source_wire_is_optional_and_never_changes_the_audio_url() {
    let mut source = crate::Source {
        id: "source".into(),
        url: "http://127.0.0.1/audio".into(),
        ..Default::default()
    };
    let wire = serde_json::to_value(&source).unwrap();
    assert!(wire.get("frameSource").is_none());
    assert!(
        serde_json::from_value::<crate::Source>(wire)
            .unwrap()
            .frame_source
            .is_none()
    );
    source.frame_source = Some("hongguo-frames://source".into());
    let wire = serde_json::to_value(&source).unwrap();
    assert_eq!(wire["frameSource"], "hongguo-frames://source");
    assert_eq!(wire["url"], "http://127.0.0.1/audio");
}

#[test]
fn revocation_rejects_a_late_result_after_the_provider_was_entered() {
    use std::{
        sync::{Condvar, mpsc},
        time::Duration,
    };
    struct InFlight {
        entered: mpsc::SyncSender<()>,
        closed: Mutex<bool>,
        changed: Condvar,
    }
    impl FrameSource for InFlight {
        fn info(&self) -> Result<Option<FrameInfo>, String> {
            self.entered.send(()).unwrap();
            let mut closed = self.closed.lock().unwrap();
            while !*closed {
                closed = self.changed.wait(closed).unwrap();
            }
            Ok(Some(FrameInfo {
                width: 2,
                height: 2,
            }))
        }
        fn frame_at(&self, _: u64) -> Result<Option<Arc<PixelFrame>>, String> {
            Ok(None)
        }
        fn seek(&self, _: u64) -> Result<u64, String> {
            Ok(1)
        }
        fn presented(&self, _: u64, _: u64) -> Result<bool, String> {
            Ok(true)
        }
        fn holding(&self) -> bool {
            true
        }
        fn close(&self) {
            *self.closed.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }
    let (entered, received) = mpsc::sync_channel(1);
    let registration = register_frames(
        &token(),
        Arc::new(InFlight {
            entered,
            closed: Mutex::new(false),
            changed: Condvar::new(),
        }),
    )
    .unwrap();
    let consumer = open_frames(registration.url()).unwrap();
    let worker = std::thread::spawn(move || consumer.info());
    received.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(registration);
    assert!(worker.join().unwrap().is_err());
}
