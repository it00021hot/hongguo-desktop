//! ffmpeg 转码执行。
//!
//! 探测与编码器选择在 [super::ffmpeg_probe]；这里只负责真正跑转码。

use std::process::Command;

use super::probe::ffmpeg_path;

/// 转码一集：有 ffmpeg 走 ffmpeg，否则返回 `None` 让调用方回落软解。
///
/// 参数对齐现版：H.264（crf 23）+ AAC 128k + yuv420p。
pub fn transcode_with_ffmpeg(
    input: &std::path::Path,
    output: &std::path::Path,
    encoder: &str,
) -> crate::error::AppResult<()> {
    use crate::error::AppError;

    let Some(ffmpeg) = ffmpeg_path() else {
        return Err(AppError::Media("没有 ffmpeg".into()));
    };

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = crate::service::download_service::worker::temp_path_for(output);

    // 编码器专属质量参数：硬件编码器用 cq/QP 语义，软件用 crf
    let quality: &[&str] = match encoder {
        "h264_nvenc" => &[
            "-c:v",
            "h264_nvenc",
            "-preset",
            "p4",
            "-cq",
            "23",
            "-b:v",
            "0",
        ],
        "h264_qsv" => &["-c:v", "h264_qsv", "-global_quality", "23"],
        "h264_amf" => &[
            "-c:v", "h264_amf", "-quality", "speed", "-rc", "cqp", "-qp_i", "23", "-qp_p", "23",
        ],
        _ => &[
            "-c:v",
            encoder,
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-profile:v",
            "high",
            "-level",
            "4.2",
        ],
    };

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        input.display().to_string(),
    ];
    args.extend(quality.iter().map(|s| (*s).to_string()));
    args.extend([
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "128k".to_string(),
        temp.display().to_string(),
    ]);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = Command::new(ffmpeg)
        .args(&arg_refs)
        .output()
        .map_err(|e| AppError::Media(format!("启动 ffmpeg 失败: {e}")))?;

    if !out.status.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::Media(format!(
            "ffmpeg 转码失败: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }

    std::fs::rename(&temp, output).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::ffmpeg::probe::ffmpeg_path;

    #[test]
    fn transcode_without_ffmpeg_errors() {
        if ffmpeg_path().is_some() {
            return; // 装了 ffmpeg 就跳过这条
        }
        let r = transcode_with_ffmpeg(
            std::path::Path::new("/nope.mp4"),
            std::path::Path::new("/tmp/o.mp4"),
            "libx264",
        );
        assert!(r.is_err(), "没有 ffmpeg 时应明确报错而不是静默成功");
    }
}
