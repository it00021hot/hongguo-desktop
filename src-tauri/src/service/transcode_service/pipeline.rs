//! 转码流水线编排。
//!
//! 分流策略（整个媒体层的关键决策）：
//! - **平台原生硬编可用**（VideoToolbox / Media Foundation）→ 接近实时
//! - **装了 ffmpeg** → 用 ffmpeg，有硬件编码器时接近实时
//! - **都没有** → 纯 Rust（`rusty_h265` + `rusty_h264` + `muxide`），零外部依赖
//!
//! 三条路都产出 H.264 MP4，对上层完全一致；上游失败时自动落回下一条，
//! 不让用户卡死。实际走了哪条由 [`crate::media::Backend`] 报告，谁都不许
//! 再拿字符串猜。
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
            decoder: crate::media::capability::selected_backend()
                .decoder_label()
                .to_string(),
            encoder: String::new(),
            backend: crate::media::capability::selected_backend(),
        });
    }

    let started = std::time::Instant::now();
    let target = cache::cache_file(series_id, vid_index);

    // 分流 1：平台原生硬编（VideoToolbox / Media Foundation）。
    // 不依赖用户装任何东西，随 GPU 驱动/系统提供；探测失败会静默落回 ffmpeg。
    let mut backend = crate::media::Backend::Rust;
    if let Some(done) =
        crate::media::platform::transcode_h264(&crate::media::platform::PlatformRequest {
            input: source,
            output: &target,
            scale_to,
            on_progress,
        })
    {
        match done {
            Ok(()) => {
                if output_is_playable(&target) {
                    backend = crate::media::Backend::Platform;
                } else {
                    log::warn!("[Transcode] 平台硬编产物校验未通过，删除后尝试下一条路");
                    let _ = std::fs::remove_file(&target);
                }
            }
            Err(e) => log::warn!("[Transcode] 平台硬编失败，尝试下一条路: {e}"),
        }
    }

    // 分流 2：ffmpeg（含硬编码器）
    if backend == crate::media::Backend::Rust {
        if let Some(encoder) = crate::media::ffmpeg::h264_encoder() {
            let req = crate::media::ffmpeg::TranscodeRequest {
                input: source,
                output: &target,
                scale_to,
                on_progress,
            };
            match crate::media::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
                Ok(()) => {
                    // 退出码 0 不等于文件可播（极端场景：磁盘写满截断、被杀毒软件
                    // 半路锁文件）。合并那头有产物校验，这里对齐同一道闸。
                    if output_is_playable(&target) {
                        backend = if encoder.hardware {
                            crate::media::Backend::FfmpegHw
                        } else {
                            crate::media::Backend::FfmpegSw
                        };
                    } else {
                        log::warn!("[Transcode] ffmpeg 产物校验未通过，删除后回落软解");
                        let _ = std::fs::remove_file(&target);
                    }
                }
                Err(e) => log::warn!("[Transcode] ffmpeg 转码失败，回落软解: {e}"),
            }
        }
    }

    // 分流 3：纯 Rust 软解
    if backend == crate::media::Backend::Rust {
        // 源不是 HEVC 时在这里就把话说清楚。不加这道闸，错误会一路推迟到
        // `media::transcode` 内部才抛「不是 HEVC 轨」，用户既不知道为什么
        // 失败，也不知道装 ffmpeg 能解决。
        crate::media::codec_probe::ensure_softdecode_supported(source)?;
        // 解码器内部的 unwind 兜底在 `media::transcode` 里，这里不再重复包一层
        crate::media::transcode::transcode_file(source, &target, options)?;
    }

    cache::trim();

    let backend_info = crate::media::ffmpeg::backend_info();
    Ok(TranscodeResult {
        output_size: std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
        output_path: target.to_string_lossy().to_string(),
        elapsed_ms: started.elapsed().as_millis(),
        frames: 0,
        decoder: backend.decoder_label().to_string(),
        encoder: match backend {
            crate::media::Backend::FfmpegHw | crate::media::Backend::FfmpegSw => {
                backend_info.encoder
            }
            _ => String::new(),
        },
        backend,
    })
}

/// 某个文件的目标分辨率（宽高）。读不出来返回 `None`。
pub fn resolution_of(path: &Path) -> Option<(u32, u32)> {
    let tracks = crate::media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.width > 0 && v.info.height > 0).then_some((v.info.width, v.info.height))
}

/// 源分集的总时长（秒）。把 [`transcode`] 的「已编码秒数」回调折成集内比例
/// 时要用它做分母；读不出来（坏文件/无视频轨）返回 `None`。
pub fn episode_seconds(path: &Path) -> Option<f64> {
    let demuxed = crate::media::demux::demux_file(path).ok()?;
    let v = demuxed.video_track()?;
    let fps = v.info.average_framerate().filter(|f| *f > 0.0)?;
    let n = v.info.samples.len();
    (n > 0).then_some(n as f64 / fps)
}

/// 转码产物是否真的可播：能解复用且有视频样本。
///
/// 与合并产物校验（`merge_service::compat`）同一口径：一个「有 moov、能打开、
/// 只播得动前几秒」的坏文件用户看不出问题，只会觉得「功能有毛病」。
fn output_is_playable(path: &Path) -> bool {
    crate::media::demux::demux_file(path)
        .ok()
        .and_then(|d| d.video_track().map(|t| !t.info.samples.is_empty()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_not_playable() {
        let dir = std::env::temp_dir().join(format!("hg-pipeline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("garbage.mp4");
        std::fs::write(&p, b"definitely not an mp4").unwrap();
        assert!(!output_is_playable(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_real_mp4_with_video_samples_is_playable() {
        use crate::domain::mp4::fixtures::{mp4_with_samples, TrackPlan};
        let dir = std::env::temp_dir().join(format!("hg-pipeline-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("real.mp4");
        std::fs::write(
            &p,
            mp4_with_samples(&[
                TrackPlan::video(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]]),
                TrackPlan::audio(vec![vec![0, 0], vec![0, 0]]),
            ]),
        )
        .unwrap();
        assert!(output_is_playable(&p), "有视频样本的合法 MP4 应判可播");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_only_output_is_not_playable() {
        use crate::domain::mp4::fixtures::{mp4_with_samples, TrackPlan};
        let dir = std::env::temp_dir().join(format!("hg-pipeline-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("audio.mp4");
        std::fs::write(&p, mp4_with_samples(&[TrackPlan::audio(vec![vec![0, 0]])])).unwrap();
        assert!(!output_is_playable(&p), "没有视频样本的产物不能当转码成功");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
