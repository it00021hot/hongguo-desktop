//! M1 解通自测：明文 ByteVC2 → 内置解核全帧计数（不启 ffmpeg）。
use std::env;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4"));
    let limit: Option<usize> = env::var("HONGGUO_M1_FRAME_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok());

    println!("decoder={}", hongguo_bytevc2::active_decoder_name());
    println!("input={}", path.display());
    let source = hongguo_bytevc2::open_plaintext(&path).expect("open");
    println!("fps={}", source.fps());

    let started = Instant::now();
    let mut frames = 0usize;
    let mut first = None;
    let mut mid = None;
    let mut last = None;
    let result = source.decode(|frame| {
        if first.is_none() {
            first = Some((frame.width, frame.height, frame.pts));
        }
        frames += 1;
        if frames == 2930 {
            mid = Some(frame.pts);
        }
        last = Some((frame.width, frame.height, frame.pts));
        if let Some(n) = limit {
            if frames >= n {
                return Err(hongguo_common::AppError::Media("limit".into()));
            }
        }
        Ok(())
    });
    let wall = started.elapsed();
    match result {
        Ok(()) => println!("status=ok"),
        Err(e) if limit.is_some() && e.to_string().contains("limit") => println!("status=limited"),
        Err(e) => {
            eprintln!("status=err {e}");
            std::process::exit(1);
        }
    }
    println!("frames={frames}");
    println!("first={first:?}");
    println!("mid_pts={mid:?}");
    println!("last={last:?}");
    println!("wall_s={:.3}", wall.as_secs_f64());
    if frames > 0 {
        let fps = frames as f64 / wall.as_secs_f64().max(1e-9);
        println!("decode_fps={fps:.1}");
        println!("realtime_x={:.2}", fps / source.fps());
    }
}
