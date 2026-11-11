//! Requires `HONGGUO_ALLOW_FFMPEG=1` (ffmpeg debug fallback is off by default).
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn main() {
    let input = Path::new("/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4");
    let output =
        Path::new("/Users/milo/qa-hongguo-offline-data/compat-cache/p03-ffmpeg-grow.play.mp4");
    let _ = std::fs::remove_file(output);
    let t0 = Instant::now();
    let first = AtomicU64::new(0);
    let last = AtomicU64::new(0);
    let r = hongguo_media_platform::ffmpeg::try_transcode_bytevc2(
        &hongguo_media_platform::platform::PlatformRequest {
            input,
            output,
            scale_to: None,
            rate: hongguo_media_platform::RateProfile::Playback,
            on_progress: Some(&|pts| {
                let ms = t0.elapsed().as_millis();
                if ms % 500 < 50 {
                    eprintln!("pts={pts:.2} wall_ms={ms}");
                }
            }),
            on_playable_prefix: Some(&|_| {
                first.store(t0.elapsed().as_millis() as u64, Ordering::Relaxed);
                eprintln!("prefix_ms={}", t0.elapsed().as_millis());
            }),
            on_committed: Some(&|n| {
                last.store(n, Ordering::Relaxed);
            }),
            is_cancelled: None,
        },
    );
    match r {
        None => {
            eprintln!("ffmpeg unavailable");
            std::process::exit(2);
        }
        Some(Ok(())) => {
            eprintln!(
                "ok wall_ms={} prefix_ms={} bytes={} dur_probe=",
                t0.elapsed().as_millis(),
                first.load(Ordering::Relaxed),
                last.load(Ordering::Relaxed)
            );
        }
        Some(Err(e)) => {
            eprintln!("err={e:?}");
            std::process::exit(1);
        }
    }
}
