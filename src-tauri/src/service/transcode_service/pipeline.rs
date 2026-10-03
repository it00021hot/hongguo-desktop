//! 转码流水线编排。
//!
//! 分流策略（整个媒体层的关键决策）：
//! - **装了 ffmpeg** → 用 ffmpeg，有硬件编码器时接近实时
//! - **没装 ffmpeg** → 纯 Rust（`rusty_h265` + `rusty_h264` + `muxide`），零外部依赖
//!
//! 两条路都产出 H.264 MP4，对上层完全一致；ffmpeg 存在但这次失败时
//! 也会自动回落软解，不让用户卡死。
//!
//! 谁在用：合并功能的「兼容格式合并」（`merge_service::compat`）与播放兼容兜底。

use std::path::Path;

use crate::error::AppResult;
use crate::media::transcode::{TranscodeOptions, TranscodeResult};
use crate::service::transcode_service::cache;

/// 转码一个文件。
///
/// `scale_to` 给出时把画面统一到该分辨率——短剧各集由平台分别编码，混着不同
/// 分辨率是常态，不统一的话转码产物依然规格不一，拼接那一步照样过不去。
/// `on_progress` 收到已编码秒数，用于把进度从「第 N/M 集」细化到集内百分比。
pub fn transcode(
    series_id: &str,
    vid_index: u32,
    source: &Path,
    options: &TranscodeOptions,
    scale_to: Option<(u32, u32)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<TranscodeResult> {
    if !source.exists() {
        return Err(crate::error::AppError::NotFound(
            source.display().to_string(),
        ));
    }

    // 命中缓存直接返回
    if let Some(cached) = cache::cached_path(series_id, vid_index) {
        let meta = std::fs::metadata(&cached).ok();
        return Ok(TranscodeResult {
            output_path: cached.to_string_lossy().to_string(),
            output_size: meta.map(|m| m.len()).unwrap_or(0),
            elapsed_ms: 0,
            frames: 0,
            decoder: crate::media::backend_info().transcode_with,
            encoder: String::new(),
        });
    }

    let started = std::time::Instant::now();
    let target = cache::cache_file(series_id, vid_index);

    // 分流 1：ffmpeg（含硬编码器）
    let mut used_ffmpeg = false;
    if let Some(encoder) = crate::media::ffmpeg::h264_encoder() {
        let req = crate::media::ffmpeg::TranscodeRequest {
            input: source,
            output: &target,
            scale_to,
            on_progress,
        };
        match crate::media::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
            Ok(()) => used_ffmpeg = true,
            Err(e) => log::warn!("[Transcode] ffmpeg 转码失败，回落软解: {e}"),
        }
    }

    // 分流 2：纯 Rust 软解
    if !used_ffmpeg {
        // 源不是 HEVC 时在这里就把话说清楚。不加这道闸，错误会一路推迟到
        // `media::transcode` 内部才抛「不是 HEVC 轨」，用户既不知道为什么
        // 失败，也不知道装 ffmpeg 能解决。
        crate::media::codec_probe::ensure_softdecode_supported(source)?;
        // 解码器内部的 unwind 兜底在 `media::transcode` 里，这里不再重复包一层
        crate::media::transcode::transcode_file(source, &target, options)?;
    }

    cache::trim();

    let backend = crate::media::backend_info();
    Ok(TranscodeResult {
        output_size: std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
        output_path: target.to_string_lossy().to_string(),
        elapsed_ms: started.elapsed().as_millis(),
        frames: 0,
        decoder: if used_ffmpeg {
            backend.transcode_with
        } else {
            "rusty_h265".to_string()
        },
        encoder: if used_ffmpeg {
            backend.encoder
        } else {
            "rusty_h264".to_string()
        },
    })
}

/// 某个文件的目标分辨率（宽高）。读不出来返回 `None`。
pub fn resolution_of(path: &Path) -> Option<(u32, u32)> {
    let tracks = crate::media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.width > 0 && v.info.height > 0).then_some((v.info.width, v.info.height))
}
