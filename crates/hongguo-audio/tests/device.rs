use hongguo_audio::{Output, PcmFormat, PcmSource, ReadResult};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

struct Silence {
    epoch: AtomicU64,
    callbacks: AtomicU64,
    consumed: AtomicU64,
}

impl PcmSource for Silence {
    fn read(&self, output: &mut [f32]) -> ReadResult {
        output.fill(0.0);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        ReadResult {
            media_frames: 0,
            epoch: self.epoch.load(Ordering::Relaxed),
        }
    }
    fn played(&self, _epoch: u64, frames: u64) {
        self.consumed.store(frames, Ordering::Relaxed);
    }
}

#[test]
#[ignore = "opens an independent silent PCM output device"]
fn silent_device_reports_consumption_pause_and_epoch_flush() {
    let source = Arc::new(Silence {
        epoch: AtomicU64::new(7),
        callbacks: AtomicU64::new(0),
        consumed: AtomicU64::new(0),
    });
    let format = PcmFormat {
        sample_rate: 48000,
        channels: 2,
    };
    let mut device = Output::open(format, source.clone()).expect("PCM open");
    assert_eq!(device.format(), format);
    assert_eq!(source.callbacks.load(Ordering::Relaxed), 0);
    device.flush(7).expect("flush before start");
    device.start().expect("start");
    std::thread::sleep(Duration::from_millis(120));
    let running = device.position().expect("device clock");
    assert_eq!(running.epoch, 7);
    assert!(source.callbacks.load(Ordering::Relaxed) > 0);
    assert!(running.device_frames > 0);
    device.pause().expect("pause");
    let paused = device.position().expect("paused clock");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(device.position().expect("frozen clock"), paused);
    source.epoch.store(8, Ordering::Relaxed);
    device.flush(8).expect("new epoch");
    let flushed = device.position().expect("flushed clock");
    assert_eq!(flushed.epoch, 8);
    assert_eq!(flushed.device_frames, 0);
    device.start().expect("resume new epoch");
    std::thread::sleep(Duration::from_millis(80));
    let resumed = device.position().expect("resumed clock");
    assert_eq!(resumed.epoch, 8);
    assert!(resumed.device_frames > 0);
    assert!(source.consumed.load(Ordering::Relaxed) > 0);
}
