//! 转码流水线编排。分流策略（媒体层的关键决策）：平台原生硬编可用 → 用平台；
//! 装了 ffmpeg → 用 ffmpeg（有硬件编码器时接近实时）；都没有 → 直接报错。
//! 两条路都产出 H.264 MP4，上游失败自动落回下一条；实际走了哪条由
//! [`hongguo_media::Backend`] 报告，谁都不许再拿字符串猜。

use std::path::Path;

use crate::transcode_service::cache;
use hongguo_common::AppResult;
use hongguo_media::transcode::TranscodeResult;
use hongguo_media_platform::RateProfile;

/// 兼容播放输出长边上限（720p）。短剧手机竖屏观感够用；1080→720 像素量约 1/2.25，
/// 码率随分辨率降，缓存体积与软编耗时同量级下降。
const PLAYBACK_MAX_LONG_EDGE: u32 = 1280;

/// 转码一个文件。
///
/// `scale_to` 给出时把画面统一到该分辨率——短剧各集由平台分别编码，混着不同
/// 分辨率是常态，不统一的话转码产物依然规格不一，拼接那一步照样过不去。
/// `rate` 区分合并归档与兼容播放；播放档会走更轻的码率与（ffmpeg 软编）更快 preset。
/// `on_progress` 收到已编码秒数，用于把进度从「第 N/M 集」细化到集内百分比。
/// 播放档边写边读的回调。合并路径保持默认空。
#[derive(Clone, Copy, Default)]
pub struct TranscodeHooks<'a> {
    pub on_committed: Option<&'a (dyn Fn(u64) + Send + Sync)>,
    pub is_cancelled: Option<&'a (dyn Fn() -> bool + Send + Sync)>,
}

pub fn transcode(
    series_id: &str,
    vid_index: u32,
    source: &Path,
    scale_to: Option<(u32, u32)>,
    rate: RateProfile,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
    hooks: TranscodeHooks<'_>,
) -> AppResult<TranscodeResult> {
    if hooks.is_cancelled.is_some_and(|cancelled| cancelled()) {
        return Err(hongguo_common::AppError::Cancelled);
    }
    if !source.exists() {
        return Err(hongguo_common::AppError::NotFound(
            source.display().to_string(),
        ));
    }

    // 命中缓存直接返回（播放/合并各用各的文件名，互不踩）
    if let Some(cached) = cache::cached_path_for(series_id, vid_index, rate) {
        return Ok(TranscodeResult {
            output_path: cached.to_string_lossy().to_string(),
            backend: hongguo_media_platform::capability::selected_backend(),
        });
    }

    let target = cache::cache_file_for(series_id, vid_index, rate);

    // ByteVC2 只走内置 Rust 解帧 + 平台 H.264 编码。失败不落 ffmpeg：
    // ffmpeg 解 ByteVC2 不在这条路径上。
    if is_bytevc2_source(source) {
        log::info!("[Transcode] {series_id} 第 {vid_index} 集走 ByteVC2 内置解码");
        if let Err(err) = hongguo_media_platform::transcode_bytevc2(
            &hongguo_media_platform::platform::PlatformRequest {
                input: source,
                output: &target,
                scale_to,
                rate,
                on_progress,
                on_playable_prefix: None,
                on_committed: hooks.on_committed,
                is_cancelled: hooks.is_cancelled,
            },
        ) {
            let _ = std::fs::remove_file(&target);
            return Err(err);
        }
        if !output_is_playable(&target) {
            let _ = std::fs::remove_file(&target);
            return Err(hongguo_common::AppError::Media(
                "ByteVC2 解码产物无法播放".into(),
            ));
        }
        return Ok(TranscodeResult {
            output_path: target.to_string_lossy().to_string(),
            backend: Some(hongguo_media::Backend::Platform),
        });
    }

    // 分流 1：平台原生编码，不依赖用户装任何东西；失败静默落回 ffmpeg。
    let mut backend: Option<hongguo_media::Backend> = None;
    // 两条路都失败时，最后一个真实原因要透给用户——吞掉它就只剩一句「没找到 ffmpeg」的假话。
    let mut last_error: Option<String> = None;
    if let Some(done) = hongguo_media_platform::platform::transcode_h264(
        &hongguo_media_platform::platform::PlatformRequest {
            input: source,
            output: &target,
            scale_to,
            rate,
            on_progress,
            on_playable_prefix: None,
            on_committed: None,
            is_cancelled: hooks.is_cancelled,
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
            rate,
            on_progress,
        };
        match hongguo_media_platform::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
            Ok(()) => {
                // 退出码 0 不等于文件可播（磁盘写满截断、被杀毒软件半路锁文件），
                // 与合并那头的产物校验对齐同一道闸。
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
            Some(e) => hongguo_common::AppError::Media(format!("转码失败：{e}")),
            None => hongguo_common::AppError::Media(
                "没有可用的转码后端：系统编码器不可用，且未找到 ffmpeg。请安装 ffmpeg 后重试。"
                    .into(),
            ),
        });
    };

    log::info!(
        "[Transcode] {series_id} 第 {vid_index} 集完成，后端 {}",
        backend.decoder_label()
    );
    Ok(TranscodeResult {
        output_path: target.to_string_lossy().to_string(),
        backend: Some(backend),
    })
}

fn is_bytevc2_source(path: &Path) -> bool {
    hongguo_media::demux::demux_file(path)
        .ok()
        .and_then(|d| {
            d.video_track()
                .map(|t| hongguo_bytevc2::is_bytevc2(&t.info.codec))
        })
        .unwrap_or(false)
}

/// 某个文件的目标分辨率（宽高）。读不出来返回 `None`。
pub fn resolution_of(path: &Path) -> Option<(u32, u32)> {
    let tracks = hongguo_media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.width > 0 && v.info.height > 0).then_some((v.info.width, v.info.height))
}

/// 兼容播放缩放目标：长边封顶 [`PLAYBACK_MAX_LONG_EDGE`]，已更小则 `None`（保持源分辨率）。
/// 宽高一律收成偶数（H.264 硬性要求）。
pub fn playback_scale(src: (u32, u32)) -> Option<(u32, u32)> {
    let (w, h) = src;
    if w == 0 || h == 0 {
        return None;
    }
    let long = w.max(h);
    if long <= PLAYBACK_MAX_LONG_EDGE {
        return None;
    }
    let scale = f64::from(PLAYBACK_MAX_LONG_EDGE) / f64::from(long);
    let nw = ((f64::from(w) * scale) as u32) & !1;
    let nh = ((f64::from(h) * scale) as u32) & !1;
    Some((nw.max(2), nh.max(2)))
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

/// 转码产物是否真的可播：能解复用且有视频样本（与合并产物校验同一口径）。
fn output_is_playable(path: &Path) -> bool {
    let demuxed = hongguo_media::demux::demux_file(path);
    if demuxed
        .as_ref()
        .ok()
        .and_then(|d| d.video_track())
        .is_some_and(|t| !t.info.samples.is_empty())
    {
        return true;
    }
    if hongguo_media::mux::prefix_is_independently_decodable(path) {
        return true;
    }
    // 校验失败必须留痕：平台层产物被拒后静默落 ffmpeg，没有这行日志就查不出「为什么没用上硬编」。
    if let Err(e) = demuxed {
        log::warn!("[Transcode] 产物校验：解复用失败: {e}");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytevc2_fourcc_is_recognized_as_its_own_source() {
        use hongguo_video::fixtures::{TrackSpec, mp4};
        let dir = std::env::temp_dir().join(format!("hg-bvc2-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("b.mp4");
        std::fs::write(&path, mp4(&[TrackSpec::video(1, b"bvc2", 64, 64)])).unwrap();
        assert!(is_bytevc2_source(&path));
        let h264 = dir.join("a.mp4");
        std::fs::write(&h264, mp4(&[TrackSpec::video(1, b"avc1", 64, 64)])).unwrap();
        assert!(!is_bytevc2_source(&h264));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn playback_scale_caps_1080p_to_720p_long_edge() {
        assert_eq!(playback_scale((1920, 1080)), Some((1280, 720)));
        assert_eq!(playback_scale((1080, 1920)), Some((720, 1280)));
        assert_eq!(playback_scale((1280, 720)), None, "已是上限不动");
        assert_eq!(playback_scale((720, 1280)), None);
    }

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
