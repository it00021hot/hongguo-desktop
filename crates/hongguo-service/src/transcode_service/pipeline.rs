//! 转码流水线编排。分流策略（媒体层的关键决策）：平台原生硬编可用 → 用平台；
//! 仅当显式打开 `HONGGUO_ALLOW_FFMPEG=1` 时才用 ffmpeg（有硬件编码器时接近实时）；
//! 都没有 → 直接报错。本模块只负责合并归档。
//! 两条路都产出 HEVC MP4，上游失败自动落回下一条；实际走了哪条由
//! [`hongguo_media::Backend`] 报告，谁都不许再拿字符串猜。

use std::path::Path;

use crate::transcode_service::cache;
use hongguo_common::AppResult;
use hongguo_media::transcode::TranscodeResult;

/// 转码一个文件（兼容格式合并的分集输入）。
///
/// `scale_to` 给出时把画面统一到该分辨率——短剧各集由平台分别编码，混着不同
/// 分辨率是常态，不统一的话转码产物依然规格不一，拼接那一步照样过不去。
/// `on_progress` 收到已编码秒数，用于把进度从「第 N/M 集」细化到集内百分比。
pub fn transcode(
    source: &Path,
    target: &Path,
    scale_to: Option<(u32, u32)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<TranscodeResult> {
    if !source.exists() {
        return Err(hongguo_common::AppError::NotFound(
            source.display().to_string(),
        ));
    }

    let demuxed = hongguo_media::demux::demux_file(source)?;
    let codec = &demuxed
        .video_track()
        .ok_or_else(|| hongguo_common::AppError::Media("归档输入缺少视频轨道".into()))?
        .info
        .codec;
    if !hongguo_media::hevc::is_hevc(codec) && !hongguo_bytevc2::is_bytevc2(codec) {
        return Err(hongguo_common::AppError::CodecUnsupported(codec.clone()));
    }

    // 命中缓存直接返回
    if cache::is_complete(target) && output_is_hevc_archive(target) {
        return Ok(TranscodeResult {
            output_path: target.to_string_lossy().to_string(),
            backend: hongguo_media_platform::capability::selected_backend(),
        });
    }

    cache::discard_completion(target);

    // ByteVC2：默认走进程内 vvdec + 平台 HEVC，不拉起外部 ffmpeg。
    // ffmpeg/librempeg 仅在 HONGGUO_ALLOW_FFMPEG=1 时作为调试 fallback，
    // 且只在内置路径失败并清掉产物之后才尝试。
    if is_bytevc2_source(source) {
        let platform_req = hongguo_media_platform::platform::PlatformRequest {
            input: source,
            output: target,
            scale_to,
            on_progress,
        };
        log::info!("[Transcode] ByteVC2 内置解码: {}", target.display());
        let builtin = hongguo_media_platform::transcode_bytevc2(&platform_req);
        if builtin.is_ok() && output_is_hevc_archive(target) {
            return complete_archive(target, hongguo_media::Backend::Platform);
        }
        let builtin_err = match builtin {
            Ok(()) => {
                let _ = hongguo_store::paths::qa_remove_file(target);
                hongguo_common::AppError::Media("ByteVC2 解码产物无法播放".into())
            }
            Err(err) => {
                let _ = hongguo_store::paths::qa_remove_file(target);
                err
            }
        };
        if let Some(ff) = hongguo_media_platform::ffmpeg::try_transcode_bytevc2(&platform_req) {
            return match ff {
                Ok(()) if output_is_hevc_archive(target) => {
                    let encoder = hongguo_media_platform::ffmpeg::hevc_encoder();
                    let backend = if encoder.is_some_and(|e| e.hardware) {
                        hongguo_media::Backend::FfmpegHw
                    } else {
                        hongguo_media::Backend::FfmpegSw
                    };
                    log::info!(
                        "[Transcode] ByteVC2 走 ffmpeg 调试 fallback（{}）: {}",
                        backend.decoder_label(),
                        target.display()
                    );
                    complete_archive(target, backend)
                }
                Ok(()) => {
                    let _ = hongguo_store::paths::qa_remove_file(target);
                    Err(hongguo_common::AppError::Media(
                        "ByteVC2 ffmpeg 产物无法播放".into(),
                    ))
                }
                Err(err) => {
                    let _ = hongguo_store::paths::qa_remove_file(target);
                    Err(err)
                }
            };
        }
        return Err(builtin_err);
    }

    // 分流 1：平台原生编码，不依赖用户装任何东西；失败后仅在显式允许时落回 ffmpeg。
    let mut backend: Option<hongguo_media::Backend> = None;
    // 两条路都失败时，最后一个真实原因要透给用户——吞掉它就只剩一句「没找到 ffmpeg」的假话。
    let mut last_error: Option<String> = None;
    if let Some(done) = hongguo_media_platform::platform::transcode_hevc(
        &hongguo_media_platform::platform::PlatformRequest {
            input: source,
            output: target,
            scale_to,
            on_progress,
        },
    ) {
        match done {
            Ok(()) => {
                if output_is_hevc_archive(target) {
                    backend = Some(hongguo_media::Backend::Platform);
                } else {
                    log::warn!("[Transcode] 平台硬编产物校验未通过，删除后尝试下一条路");
                    last_error = Some("平台编码器产出的文件无法播放".to_string());
                    let _ = hongguo_store::paths::qa_remove_file(target);
                }
            }
            Err(e) => {
                log::warn!("[Transcode] 平台硬编失败，尝试下一条路: {e}");
                last_error = Some(e.to_string());
            }
        }
    }

    // 分流 2：ffmpeg 调试 fallback（含硬编码器；默认关闭，见 HONGGUO_ALLOW_FFMPEG）
    if backend.is_none()
        && let Some(encoder) = hongguo_media_platform::ffmpeg::hevc_encoder()
    {
        let req = hongguo_media_platform::ffmpeg::TranscodeRequest {
            input: source,
            output: target,
            scale_to,
            on_progress,
        };
        match hongguo_media_platform::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
            Ok(()) => {
                // 退出码 0 不等于文件可播（磁盘写满截断、被杀毒软件半路锁文件），
                // 与合并那头的产物校验对齐同一道闸。
                if output_is_hevc_archive(target) {
                    backend = Some(if encoder.hardware {
                        hongguo_media::Backend::FfmpegHw
                    } else {
                        hongguo_media::Backend::FfmpegSw
                    });
                } else {
                    log::warn!("[Transcode] ffmpeg 产物校验未通过，删除产物");
                    last_error = Some("ffmpeg 产出的文件无法播放".to_string());
                    let _ = hongguo_store::paths::qa_remove_file(target);
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
                "没有可用的转码后端：系统编码器不可用，且未启用 ffmpeg 调试 fallback（需显式设置 HONGGUO_ALLOW_FFMPEG=1）。"
                    .into(),
            ),
        });
    };

    log::info!(
        "[Transcode] 转码完成，后端 {}: {}",
        backend.decoder_label(),
        target.display()
    );
    complete_archive(target, backend)
}

fn complete_archive(target: &Path, backend: hongguo_media::Backend) -> AppResult<TranscodeResult> {
    cache::seal_complete(target)
        .map_err(|error| hongguo_common::AppError::Io(error.to_string()))?;
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

/// 源分集的总时长（秒）。把 [`transcode`] 的「已编码秒数」回调折成集内比例
/// 时要用它做分母；读不出来（坏文件/无视频轨）返回 `None`。
pub fn episode_seconds(path: &Path) -> Option<f64> {
    let demuxed = hongguo_media::demux::demux_file(path).ok()?;
    let v = demuxed.video_track()?;
    let fps = v.info.average_framerate().filter(|f| *f > 0.0)?;
    let n = v.info.samples.len();
    (n > 0).then_some(n as f64 / fps)
}

pub(crate) fn output_is_hevc_archive(path: &Path) -> bool {
    let demuxed = hongguo_media::demux::demux_file(path);
    if let Ok(movie) = &demuxed {
        return movie.video_track().is_some_and(|track| {
            matches!(track.info.codec.as_str(), "hvc1" | "hev1") && !track.info.samples.is_empty()
        });
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
    fn avc_input_is_rejected_before_cache_or_codec_backend() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4");
        let target = std::env::temp_dir().join(format!("hg-reject-avc-{}.mp4", std::process::id()));
        assert!(matches!(
            transcode(&source, &target, None, None),
            Err(hongguo_common::AppError::CodecUnsupported(_))
        ));
        assert!(!target.exists());
    }

    #[test]
    fn avc_file_cannot_pass_hevc_archive_output_validation() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4");
        assert!(!output_is_hevc_archive(&path));
    }

    #[test]
    fn hevc_main_and_main10_pass_archive_output_validation() {
        for name in [
            "hevc-main-136x76-aac.mp4",
            "hevc-main10-128x72-hev1-aac.mp4",
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../hongguo-softdec/tests/fixtures")
                .join(name);
            assert!(output_is_hevc_archive(&path));
        }
    }

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
    fn garbage_is_not_playable() {
        let dir = std::env::temp_dir().join(format!("hg-pipeline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("garbage.mp4");
        std::fs::write(&p, b"definitely not an mp4").unwrap();
        assert!(!output_is_hevc_archive(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_only_output_is_not_playable() {
        use hongguo_video::fixtures::{TrackPlan, mp4_with_samples};
        let dir = std::env::temp_dir().join(format!("hg-pipeline-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("audio.mp4");
        std::fs::write(&p, mp4_with_samples(&[TrackPlan::audio(vec![vec![0, 0]])])).unwrap();
        assert!(
            !output_is_hevc_archive(&p),
            "没有视频样本的产物不能当转码成功"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
