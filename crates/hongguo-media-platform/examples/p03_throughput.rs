//! P0-3: measure ByteVC2 progressive commit cadence (media secs vs wall).
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn main() {
    let input = std::env::args().nth(1).unwrap_or_else(|| {
        "/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4".into()
    });
    let out = std::env::args().nth(2).unwrap_or_else(|| {
        format!(
            "/Users/milo/qa-hongguo-offline-data/compat-cache/p03-bench-{}.play.mp4",
            std::process::id()
        )
    });
    let input = Path::new(&input);
    let output = Path::new(&out);
    let _ = std::fs::remove_file(output);
    let _ = std::fs::remove_file(format!("{}.complete", output.display()));

    let t0 = Instant::now();
    let commits: Mutex<Vec<(u128, u64)>> = Mutex::new(Vec::new());
    let prefix_ns = AtomicU64::new(0);
    let last_log = AtomicU64::new(0);

    let r = hongguo_media_platform::transcode_bytevc2(
        &hongguo_media_platform::platform::PlatformRequest {
            input,
            output,
            scale_to: None, // 1280x720 already at playback cap
            rate: hongguo_media_platform::RateProfile::Playback,
            on_progress: Some(&|pts| {
                let ms = t0.elapsed().as_millis() as u64;
                let prev = last_log.load(Ordering::Relaxed);
                if ms.saturating_sub(prev) >= 2000 || (90.0..91.0).contains(&pts) {
                    last_log.store(ms, Ordering::Relaxed);
                    let factor = if ms > 0 {
                        pts / (ms as f64 / 1000.0)
                    } else {
                        0.0
                    };
                    eprintln!("progress pts={pts:.2} wall_ms={ms} x={factor:.3}");
                }
            }),
            on_playable_prefix: Some(&|_| {
                prefix_ns.store(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                eprintln!("prefix_ready_ms={}", t0.elapsed().as_millis());
            }),
            on_committed: Some(&|bytes| {
                let ms = t0.elapsed().as_millis();
                commits.lock().unwrap().push((ms, bytes));
            }),
            is_cancelled: None,
        },
    );
    match r {
        Ok(()) => eprintln!("ok full_wall_ms={}", t0.elapsed().as_millis()),
        Err(e) => {
            eprintln!("err={e:?} wall_ms={}", t0.elapsed().as_millis());
            std::process::exit(1);
        }
    }
    let c = commits.lock().unwrap();
    eprintln!("commits={}", c.len());
    if let Some((ms, b)) = c.first() {
        eprintln!("first_commit_ms={ms} bytes={b}");
    }
    // approximate: sample every ~30 commits around mid
    for (i, (ms, b)) in c.iter().enumerate() {
        if i % 30 == 0 || i + 1 == c.len() {
            eprintln!("commit[{i}] ms={ms} bytes={b}");
        }
    }
    let meta = std::fs::metadata(output).ok();
    eprintln!(
        "out_bytes={} prefix_ms={}",
        meta.map(|m| m.len()).unwrap_or(0),
        prefix_ns.load(Ordering::Relaxed) / 1_000_000
    );
}
