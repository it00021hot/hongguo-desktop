//! 转码流水线编排。
//!
//! 分流策略（整个媒体层的关键决策）：
//! - **装了 ffmpeg** → 用 ffmpeg，可用 NVENC/QSV/AMF 硬编码，接近实时
//! - **没装 ffmpeg** → 纯 Rust（`rusty_h265` + `rusty_h264` + `muxide`），零外部依赖
//!
//! 两条路都产出 H.264 MP4，对上层完全一致；ffmpeg 存在但这次失败时
//! 也会自动回落软解，不让用户卡死。
//!
//! 谁在用：合并功能的「兼容格式合并」（`merge_service::compat`）。
//! 播放兜底已下线。

use std::path::Path;

use crate::error::AppResult;
use crate::media::transcode::{TranscodeOptions, TranscodeResult};
use crate::service::transcode_service::cache;

/// 转码一个文件。
pub fn transcode(
    series_id: &str,
    vid_index: u32,
    source: &Path,
    options: &TranscodeOptions,
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
        match crate::media::ffmpeg::transcode_with_ffmpeg(source, &target, encoder) {
            Ok(()) => used_ffmpeg = true,
            Err(e) => log::warn!("[Transcode] ffmpeg 转码失败，回落软解: {e}"),
        }
    }

    // 分流 2：纯 Rust 软解
    if !used_ffmpeg {
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
