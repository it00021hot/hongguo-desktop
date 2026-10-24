//! 转码流水线编排。
//!
//! 分流策略（整个媒体层的关键决策）：
//! - **平台原生硬编可用**（VideoToolbox / Media Foundation）→ 接近实时
//! - **装了 ffmpeg** → 用 ffmpeg，有硬件编码器时接近实时
//! - **都没有** → 做不了转码，直接报错
//!
//! 两条路都产出 H.264 MP4，对上层完全一致；上游失败时自动落回下一条，
//! 不让用户卡死。实际走了哪条由 [`hongguo_media::Backend`] 报告，谁都不许
//! 再拿字符串猜。
//!
//! 谁在用：合并功能的「兼容格式合并」（`merge_service::compat`）与播放兼容兜底。

use std::path::Path;

use crate::transcode_service::cache;
use hongguo_common::AppResult;
use hongguo_media::transcode::TranscodeResult;

/// 转码一个文件。
///
/// `scale_to` 给出时把画面统一到该分辨率——短剧各集由平台分别编码，混着不同
/// 分辨率是常态，不统一的话转码产物依然规格不一，拼接那一步照样过不去。
/// `on_progress` 收到已编码秒数，用于把进度从「第 N/M 集」细化到集内百分比。
pub fn transcode(
    series_id: &str,
    vid_index: u32,
    source: &Path,
    scale_to: Option<(u32, u32)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<TranscodeResult> {
    if !source.exists() {
        return Err(hongguo_common::AppError::NotFound(
            source.display().to_string(),
        ));
    }

    // 命中缓存直接返回
    if let Some(cached) = cache::cached_path(series_id, vid_index) {
        return Ok(TranscodeResult {
            output_path: cached.to_string_lossy().to_string(),
            backend: hongguo_media_platform::capability::selected_backend(),
        });
    }

    let target = cache::cache_file(series_id, vid_index);

    // 分流 1：平台原生编码（macOS：VideoToolbox，硬编优先、Apple 软编回落；
    // Windows：Media Foundation 硬编）。不依赖用户装任何东西；
    // 会话建不出来/失败会静默落回 ffmpeg。
    let mut backend: Option<hongguo_media::Backend> = None;
    // 两条路都试过但都没成时，最后一个真实原因要透给用户——没有第三档兜底了，
    // 吞掉它就只剩一句「没找到 ffmpeg」的假话。
    let mut last_error: Option<String> = None;
    if let Some(done) = hongguo_media_platform::platform::transcode_h264(
        &hongguo_media_platform::platform::PlatformRequest {
            input: source,
            output: &target,
            scale_to,
            on_progress,
        },
    ) {
        match done {
            Ok(()) => {
                if output_is_playable(&target) {
                    backend = Some(hongguo_media::Backend::Platform);
                } else {
                    log::warn!("[Transcode] 平台硬编产物校验未通过，删除后尝试下一条路");
                    last_error = Some("平台编码器产出的文件无法播放".to_string());
                    let _ = std::fs::remove_file(&target);
                }
            }
            Err(e) => {
                log::warn!("[Transcode] 平台硬编失败，尝试下一条路: {e}");
                last_error = Some(e.to_string());
            }
        }
    }

    // 分流 2：ffmpeg（含硬编码器）
    if backend.is_none()
        && let Some(encoder) = hongguo_media_platform::ffmpeg::h264_encoder()
    {
        let req = hongguo_media_platform::ffmpeg::TranscodeRequest {
            input: source,
            output: &target,
            scale_to,
            on_progress,
        };
        match hongguo_media_platform::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
            Ok(()) => {
                // 退出码 0 不等于文件可播（极端场景：磁盘写满截断、被杀毒软件
                // 半路锁文件）。合并那头有产物校验，这里对齐同一道闸。
                if output_is_playable(&target) {
                    backend = Some(if encoder.hardware {
                        hongguo_media::Backend::FfmpegHw
                    } else {
                        hongguo_media::Backend::FfmpegSw
                    });
                } else {
                    log::warn!("[Transcode] ffmpeg 产物校验未通过，删除产物");
                    last_error = Some("ffmpeg 产出的文件无法播放".to_string());
                    let _ = std::fs::remove_file(&target);
                }
            }
            Err(e) => {
                log::warn!("[Transcode] ffmpeg 转码失败: {e}");
                last_error = Some(e.to_string());
            }
        }
    }

    let Some(backend) = backend else {
        return Err(match last_error {
            // 两条路都试过了但都没成：把真实原因带出去，别让用户去猜。
            Some(e) => hongguo_common::AppError::Media(format!("转码失败：{e}")),
            // 一条路都没有：机器上没有任何可用后端。
            None => hongguo_common::AppError::Media(
                "没有可用的转码后端：系统编码器不可用，且未找到 ffmpeg。请安装 ffmpeg 后重试。"
                    .into(),
            ),
        });
    };

    cache::trim();

    log::info!(
        "[Transcode] {series_id} 第 {vid_index} 集完成，后端 {}",
        backend.decoder_label()
    );
    Ok(TranscodeResult {
        output_path: target.to_string_lossy().to_string(),
        backend: Some(backend),
    })
}

/// 某个文件的目标分辨率（宽高）。读不出来返回 `None`。
pub fn resolution_of(path: &Path) -> Option<(u32, u32)> {
    let tracks = hongguo_media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.width > 0 && v.info.height > 0).then_some((v.info.width, v.info.height))
}

/// 源分集的总时长（秒）。把 [`transcode`] 的「已编码秒数」回调折成集内比例
/// 时要用它做分母；读不出来（坏文件/无视频轨）返回 `None`。
pub fn episode_seconds(path: &Path) -> Option<f64> {
    let demuxed = hongguo_media::demux::demux_file(path).ok()?;
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
    match hongguo_media::demux::demux_file(path) {
        Ok(d) => d.video_track().is_some_and(|t| !t.info.samples.is_empty()),
        // 校验失败必须留痕：平台层产物被拒后静默落 ffmpeg，没有这行日志
        // 就只剩「为什么没用上硬编」的悬案（B 帧 ctts 形态曾踩过）
        Err(e) => {
            log::warn!("[Transcode] 产物校验：解复用失败: {e}");
            false
        }
    }
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
        use hongguo_video::fixtures::{TrackPlan, mp4_with_samples};
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
        use hongguo_video::fixtures::{TrackPlan, mp4_with_samples};
        let dir = std::env::temp_dir().join(format!("hg-pipeline-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("audio.mp4");
        std::fs::write(&p, mp4_with_samples(&[TrackPlan::audio(vec![vec![0, 0]])])).unwrap();
        assert!(!output_is_playable(&p), "没有视频样本的产物不能当转码成功");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
