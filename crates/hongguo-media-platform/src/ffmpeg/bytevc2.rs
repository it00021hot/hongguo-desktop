//! ByteVC2 → H.264 via ffmpeg when the local build can decode `bvc2` (librempeg / patched ffmpeg).
//!
//! Built-in path (in-process vvdec + platform H.264) is the default playback / speed route.
//! This ffmpeg/librempeg `bvc2` path is a debug fallback only: it stays off unless
//! `HONGGUO_ALLOW_FFMPEG=1` is set (and `HONGGUO_NO_FFMPEG` is not). oxideav remains an
//! optional decode backend.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::video::RateProfile;

use super::probe::{Encoder, ffmpeg_path, h264_encoder};
use super::transcode::quality_args_for;
use crate::platform::PlatformRequest;

/// Try ffmpeg for ByteVC2. Returns `None` when the debug fallback is off (default) or
/// ffmpeg/encoder is unavailable, so the caller can use the in-process vvdec path.
pub fn try_transcode_bytevc2(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let encoder = h264_encoder()?;
    let ffmpeg = ffmpeg_path()?;
    Some(run(req, &ffmpeg, &encoder))
}

fn run(req: &PlatformRequest<'_>, ffmpeg: &Path, encoder: &Encoder) -> AppResult<()> {
    if let Some(parent) = req.output.parent() {
        hongguo_common::offline_qa::qa_create_dir_all(parent)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    hongguo_common::offline_qa::prepare_qa_io(req.output).map_err(AppError::Io)?;

    if req.on_committed.is_some() {
        run_growing(req, ffmpeg, encoder)
    } else {
        let legacy = super::transcode::TranscodeRequest {
            input: req.input,
            output: req.output,
            scale_to: req.scale_to,
            rate: req.rate,
            on_progress: req.on_progress,
        };
        super::transcode::transcode_with_ffmpeg(&legacy, encoder)
    }
}

fn run_growing(req: &PlatformRequest<'_>, ffmpeg: &Path, encoder: &Encoder) -> AppResult<()> {
    let _ = hongguo_common::offline_qa::qa_remove_file(req.output);
    let args = growing_args(req, encoder, req.output);

    let mut cmd = Command::new(ffmpeg);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(super::probe::CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Media(format!("启动 ffmpeg(ByteVC2) 失败: {e}")))?;

    let done = AtomicBool::new(false);
    let committed = AtomicU64::new(0);
    let prefix_sent = AtomicBool::new(false);
    let err_text = std::sync::Mutex::new(String::new());

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let status = std::thread::scope(|s| {
        s.spawn(|| {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if done.load(Ordering::Relaxed) || req.is_cancelled.is_some_and(|c| c()) {
                    break;
                }
                bump_commit(
                    req.output,
                    &committed,
                    &prefix_sent,
                    req.on_committed,
                    req.on_playable_prefix,
                );
                let Some(us) = line.strip_prefix("out_time_us=") else {
                    continue;
                };
                let Ok(us) = us.trim().parse::<u64>() else {
                    continue;
                };
                if let Some(cb) = req.on_progress {
                    cb(us as f64 / 1e6);
                }
            }
        });
        s.spawn(|| {
            let mut owned = String::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                owned.push_str(&line);
                owned.push('\n');
                if owned.len() > 8 * 1024 {
                    break;
                }
            }
            if let Ok(mut g) = err_text.lock() {
                *g = owned;
            }
        });
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if req.is_cancelled.is_some_and(|c| c()) {
                    break;
                }
                bump_commit(
                    req.output,
                    &committed,
                    &prefix_sent,
                    req.on_committed,
                    req.on_playable_prefix,
                );
                std::thread::sleep(Duration::from_millis(40));
            }
            bump_commit(
                req.output,
                &committed,
                &prefix_sent,
                req.on_committed,
                req.on_playable_prefix,
            );
        });

        let result = wait_child(&mut child, req.is_cancelled);
        done.store(true, Ordering::Relaxed);
        result
    });

    match status {
        Ok(()) => {
            if let Ok(meta) = std::fs::metadata(req.output)
                && let Some(cb) = req.on_committed
            {
                cb(meta.len());
            }
            Ok(())
        }
        Err(AppError::Cancelled) => {
            let _ = hongguo_common::offline_qa::qa_remove_file(req.output);
            Err(AppError::Cancelled)
        }
        Err(other) => {
            let detail = err_text
                .lock()
                .map(|g| g.trim().to_string())
                .unwrap_or_default();
            let _ = hongguo_common::offline_qa::qa_remove_file(req.output);
            if detail.is_empty() {
                Err(other)
            } else {
                Err(AppError::Media(format!(
                    "ffmpeg(ByteVC2) 转码失败: {detail}"
                )))
            }
        }
    }
}

fn bump_commit(
    output: &Path,
    committed: &AtomicU64,
    prefix_sent: &AtomicBool,
    on_committed: Option<&(dyn Fn(u64) + Send + Sync)>,
    on_prefix: Option<&(dyn Fn(&Path) + Send + Sync)>,
) {
    let Ok(meta) = std::fs::metadata(output) else {
        return;
    };
    let len = meta.len();
    if len < 2048 {
        return;
    }
    let prev = committed.load(Ordering::Relaxed);
    if len <= prev {
        return;
    }
    committed.store(len, Ordering::Relaxed);
    if let Some(cb) = on_committed {
        cb(len);
    }
    if prefix_sent
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_ok()
        && let Some(cb) = on_prefix
    {
        cb(output);
    }
}

fn wait_child(
    child: &mut Child,
    is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
) -> AppResult<()> {
    loop {
        if is_cancelled.is_some_and(|c| c()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(AppError::Media("ffmpeg(ByteVC2) 转码失败".into())),
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(AppError::Media(format!("等待 ffmpeg 失败: {e}"))),
        }
    }
}

fn growing_args(req: &PlatformRequest<'_>, encoder: &Encoder, out: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        req.input.display().to_string(),
    ];
    args.extend(quality_args_for(encoder, req.rate));
    if let Some((w, h)) = req
        .scale_to
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| (even(w), even(h)))
    {
        args.extend(["-vf".into(), format!("scale={w}:{h}")]);
    }
    if req.rate == RateProfile::Playback {
        let (w, h) = req
            .scale_to
            .map(|(w, h)| (even(w), even(h)))
            .unwrap_or((1280, 720));
        let bps = hongguo_media_platform_api::video::bitrate_for_profile(
            w as usize,
            h as usize,
            30.0,
            RateProfile::Playback,
        );
        args.extend([
            "-maxrate".into(),
            bps.to_string(),
            "-bufsize".into(),
            (bps.saturating_mul(2)).to_string(),
        ]);
    }
    args.extend([
        "-g".into(),
        "30".into(),
        "-keyint_min".into(),
        "30".into(),
        "-sc_threshold".into(),
        "0".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "copy".into(),
        "-movflags".into(),
        "frag_keyframe+empty_moov+default_base_moof+delay_moov".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-f".into(),
        "mp4".into(),
        out.display().to_string(),
    ]);
    args
}

fn even(v: u32) -> u32 {
    if v.is_multiple_of(2) {
        v
    } else {
        v - 1
    }
}
