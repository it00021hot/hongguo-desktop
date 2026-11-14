use super::*;
use hongguo_native_clock::Control;
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let mut data = std::env::var_os("HONGGUO_NATIVE_AUDIO_PROBE_SOURCE").map_or_else(
            || {
                include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4")
                    .to_vec()
            },
            |path| std::fs::read(path).expect("read explicit native audio probe source"),
        );
        hongguo_video::audio_view::AudioView::from_moov(0, &data)
            .unwrap()
            .unwrap()
            .apply(0, &mut data);
        let path = std::env::temp_dir().join(format!(
            "hg-native-clock-{}-{}.mp4",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(&data)
            .unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

fn wait(clock: &dyn Clock, predicate: impl Fn(Position) -> bool) -> Position {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let position = clock.position().unwrap();
        if predicate(position) {
            return position;
        }
        assert!(
            Instant::now() < deadline,
            "native AVPlayer clock did not reach the requested state: {position:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn run() {
    let fixture = Fixture::new();
    let url = std::env::var("HONGGUO_NATIVE_AUDIO_PROBE_BASE_URL").map_or_else(
        |_| format!("file://{}", fixture.0.display()),
        |base| {
            format!(
                "{base}/{}",
                fixture.0.file_name().unwrap().to_str().unwrap()
            )
        },
    );
    let clock = open(url).unwrap();
    let control = Control {
        volume: 0.0,
        muted: true,
        ..Default::default()
    };
    clock.control(control).unwrap();
    wait(clock.as_ref(), |state| state.ready && !state.buffering);
    clock
        .control(Control {
            playing: true,
            rate: 1.5,
            ..control
        })
        .unwrap();
    wait(clock.as_ref(), |state| state.seconds >= 0.2);
    clock.control(control).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    clock.position().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    let paused = clock.position().unwrap().seconds;
    std::thread::sleep(Duration::from_millis(50));
    clock.position().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert!((clock.position().unwrap().seconds - paused).abs() < 0.005);
    for rate in [0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
        clock.seek(0.4).unwrap();
        wait(clock.as_ref(), |state| {
            state.ready && !state.seeking && (state.seconds - 0.4).abs() < 0.02
        });
        clock
            .control(Control {
                playing: true,
                rate,
                ..control
            })
            .unwrap();
        wait(clock.as_ref(), |state| state.seconds >= 0.6);
        clock.control(control).unwrap();
    }
    clock.seek(0.7).unwrap();
    wait(clock.as_ref(), |state| {
        state.ready && !state.seeking && (state.seconds - 0.7).abs() < 0.02
    });
    clock
        .control(Control {
            playing: true,
            rate: 2.0,
            ..control
        })
        .unwrap();
    wait(clock.as_ref(), |state| state.seconds >= 0.9);
    clock.close();
    assert!(clock.position().is_err());
}
