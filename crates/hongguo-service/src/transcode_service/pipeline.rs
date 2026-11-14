use std::path::Path;

use crate::transcode_service::cache;
use hongguo_common::{AppError, AppResult};
use hongguo_media::transcode::TranscodeResult;

pub fn transcode(
    source: &Path,
    target: &Path,
    scale_to: Option<(u32, u32)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<TranscodeResult> {
    if !source.exists() {
        return Err(AppError::NotFound(source.display().to_string()));
    }
    let demuxed = hongguo_media::demux::demux_file(source)?;
    let codec = &demuxed
        .video_track()
        .ok_or_else(|| AppError::Media("归档输入缺少视频轨道".into()))?
        .info
        .codec;
    let bytevc2 = hongguo_bytevc2::is_bytevc2(codec);
    if !hongguo_media::hevc::is_hevc(codec) && !bytevc2 {
        return Err(AppError::CodecUnsupported(codec.clone()));
    }
    if cache::is_complete(target) && output_is_hevc_archive(target) {
        return Ok(TranscodeResult {
            output_path: target.to_string_lossy().to_string(),
            backend: hongguo_media_platform::capability::selected_backend(),
        });
    }
    cache::discard_completion(target);
    let request = hongguo_media_platform::platform::PlatformRequest {
        input: source,
        output: target,
        scale_to,
        on_progress,
    };
    let result = if bytevc2 {
        hongguo_media_platform::transcode_bytevc2(&request)
    } else {
        hongguo_media_platform::platform::transcode_hevc(&request).unwrap_or_else(|| {
            Err(AppError::Media(
                "当前设备没有可用的 HEVC 归档编解码器".into(),
            ))
        })
    };
    if let Err(error) = result {
        let _ = hongguo_store::paths::qa_remove_file(target);
        return Err(error);
    }
    if !output_is_hevc_archive(target) {
        let _ = hongguo_store::paths::qa_remove_file(target);
        return Err(AppError::Media("平台编码器产出的 HEVC 归档校验失败".into()));
    }
    log::info!("[Transcode] 平台归档完成: {}", target.display());
    complete_archive(target, hongguo_media::Backend::Platform)
}

fn complete_archive(target: &Path, backend: hongguo_media::Backend) -> AppResult<TranscodeResult> {
    cache::seal_complete(target)
        .map_err(|error| hongguo_common::AppError::Io(error.to_string()))?;
    Ok(TranscodeResult {
        output_path: target.to_string_lossy().to_string(),
        backend: Some(backend),
    })
}

#[cfg(test)]
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
