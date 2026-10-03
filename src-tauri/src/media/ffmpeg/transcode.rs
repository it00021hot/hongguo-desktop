//! ffmpeg 转码执行。
//!
//! 探测与编码器选择在 [`super::probe`]；这里只负责真正跑转码。

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use super::probe::{ffmpeg_path, Encoder};
use crate::error::{AppError, AppResult};

/// 一次转码的输入。
pub struct TranscodeRequest<'a> {
    /// 已解密的源 MP4
    pub input: &'a Path,
    /// 输出路径（内部先写临时文件，成功后原子替换）
    pub output: &'a Path,
    /// 统一到这个分辨率。`None` 表示保持源分辨率。
    ///
    /// 短剧各集由平台**分别**编码，同一部剧里混着 1080p 与 720p 是常态。
    /// 合并要求各集规格一致，逐集转码却不缩放的话，产物依然规格不一，拼接
    /// 那一步照样过不去。兼容合并因此按第 1 集的分辨率统一。
    pub scale_to: Option<(u32, u32)>,
    /// 已编码秒数的回调，用于把进度从「第 N/M 集」细化到集内百分比。
    pub on_progress: Option<&'a (dyn Fn(f64) + Send + Sync)>,
}

/// 转码一集。
///
/// 视频参数对齐现版：H.264 + yuv420p。音轨是**直通**（`-c:a copy`），与软解路径
/// 的 AAC 搬运保持一致——源音轨解密后已是明文 AAC，重新编码既慢又多一轮有损压缩，
/// 还会让「装不装 ffmpeg」产出两种不同的音频。
pub fn transcode_with_ffmpeg(req: &TranscodeRequest<'_>, encoder: &Encoder) -> AppResult<()> {
    let Some(ffmpeg) = ffmpeg_path() else {
        return Err(AppError::Media("没有 ffmpeg".into()));
    };

    if let Some(parent) = req.output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = crate::service::download_service::worker::temp_path_for(req.output);

    let args = build_args(req, encoder, &temp);

    let mut child = Command::new(ffmpeg)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Media(format!("启动 ffmpeg 失败: {e}")))?;

    // `-progress pipe:1` 把 `out_time_us=1234567` 这样的行写到 stdout。
    // 这里必须边跑边读：等 `output()` 收完再解析就只剩下最终结果，进度全丢了。
    //
    // 用 scoped thread 而不是 `spawn`：进度回调是调用方借给我们的引用，
    // `spawn` 的 `'static` 边界过不去。
    let stdout = child.stdout.take().expect("已 piped");
    let on_progress = req.on_progress;
    std::thread::scope(|s| {
        s.spawn(move || {
            let mut last = 0.0f64;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(us) = line.strip_prefix("out_time_us=") else {
                    continue;
                };
                let Ok(us) = us.trim().parse::<u64>() else {
                    continue;
                };
                let secs = us as f64 / 1e6;
                if secs > last {
                    last = secs;
                    if let Some(cb) = on_progress {
                        cb(secs);
                    }
                }
            }
        });
        let out = child.wait_with_output().map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            AppError::Media(format!("等待 ffmpeg 失败: {e}"))
        })?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&temp);
            return Err(AppError::Media(format!(
                "ffmpeg 转码失败: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(())
    })?;

    std::fs::rename(&temp, req.output).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;
    Ok(())
}

/// 拼出 ffmpeg 参数。
fn build_args(req: &TranscodeRequest<'_>, encoder: &Encoder, temp: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        req.input.display().to_string(),
    ];
    args.extend(quality_args(encoder));

    if let Some((w, h)) = req.scale_to.filter(|(w, h)| *w > 0 && *h > 0) {
        // H.264 要求宽高为偶数，否则 ffmpeg 直接拒绝
        let (w, h) = (even(w), even(h));
        args.extend(["-vf".to_string(), format!("scale={w}:{h}")]);
    }

    args.extend([
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-c:a".to_string(),
        "copy".to_string(),
        // moov 提到文件头。不加的话 ffmpeg 把它写在末尾，播放器必须先 seek
        // 到几十 MB 的末尾才起播；解析层只读文件头时也根本找不到 moov。
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-progress".to_string(),
        "pipe:1".to_string(),
        "-nostats".to_string(),
        // 临时文件名是 `xxx.enc.tmp`，ffmpeg 从扩展名推断不出封装格式，
        // 不显式指定就直接拒绝开输出文件——这条路径此前从未真正跑通过。
        "-f".to_string(),
        "mp4".to_string(),
        temp.display().to_string(),
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

/// 编码器专属质量参数。
///
/// 硬件分支都用「QP 越低越好」的 0–51 语义表达 crf 23；MediaFoundation 编码器
/// 不吃 `-crf` / `-preset` / `-profile`，它的质量位是 0–100 且**越高越好**。
///
/// `h264_mf` 的 60 是实测标定的：同一集跑 VMAF，`-quality 60` 得 93.03，
/// 与 `libx264 -crf 23` 的 92.97 基本重合（q50 只有 89.70）。
fn quality_args(encoder: &Encoder) -> Vec<String> {
    let owned: &[&str] = match encoder.name {
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
        "h264_mf" => &[
            "-c:v",
            "h264_mf",
            "-rate_control",
            "quality",
            "-quality",
            "60",
        ],
        _ => &[
            "-c:v",
            encoder.name,
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
    owned.iter().map(|s| (*s).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::ffmpeg::h264_encoder;

    #[test]
    fn transcode_without_ffmpeg_errors() {
        if h264_encoder().is_some() {
            return; // 装了 ffmpeg 就跳过这条
        }
        let r = transcode_with_ffmpeg(
            &TranscodeRequest {
                input: Path::new("/nope.mp4"),
                output: Path::new("/tmp/o.mp4"),
                scale_to: None,
                on_progress: None,
            },
            &Encoder {
                name: "libx264",
                hardware: false,
            },
        );
        assert!(r.is_err(), "没有 ffmpeg 时应明确报错而不是静默成功");
    }

    #[test]
    fn scale_is_emitted_only_when_requested() {
        let enc = Encoder {
            name: "libx264",
            hardware: false,
        };
        let plain = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: None,
                on_progress: None,
            },
            &enc,
            Path::new("/t.tmp"),
        );
        assert!(!plain.iter().any(|a| a == "-vf"), "没要求缩放就不该加 -vf");

        let scaled = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: Some((1920, 1080)),
                on_progress: None,
            },
            &enc,
            Path::new("/t.tmp"),
        );
        let vf = scaled
            .windows(2)
            .find(|w| w[0] == "-vf")
            .map(|w| w[1].clone());
        assert_eq!(vf.as_deref(), Some("scale=1920:1080"));
    }

    #[test]
    fn odd_dimensions_are_rounded_down_to_even() {
        // H.264 不接受奇数宽高，这里必须挡住而不是交给 ffmpeg 报错
        let enc = Encoder {
            name: "libx264",
            hardware: false,
        };
        let args = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: Some((1281, 719)),
                on_progress: None,
            },
            &enc,
            Path::new("/t.tmp"),
        );
        let vf = args
            .windows(2)
            .find(|w| w[0] == "-vf")
            .map(|w| w[1].clone());
        assert_eq!(vf.as_deref(), Some("scale=1280:718"));
    }

    #[test]
    fn the_output_format_is_always_explicit() {
        // 临时文件是 .enc.tmp，ffmpeg 推断不出容器；不写 -f mp4 就打不开输出
        let args = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: None,
                on_progress: None,
            },
            &Encoder {
                name: "libx264",
                hardware: false,
            },
            Path::new("/o.enc.tmp"),
        );
        let i = args.iter().position(|a| a == "-f").expect("必须有 -f");
        assert_eq!(args[i + 1], "mp4");
    }

    #[test]
    fn the_output_must_be_faststart() {
        // moov 写在末尾时播放器要先 seek 到文件末尾才起播，解析层读文件头
        // 也找不到 moov。产物是要单独拿去播的，必须提到前面。
        let args = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: None,
                on_progress: None,
            },
            &Encoder {
                name: "libx264",
                hardware: false,
            },
            Path::new("/t.tmp"),
        );
        let i = args
            .iter()
            .position(|a| a == "-movflags")
            .expect("必须有 -movflags");
        assert_eq!(args[i + 1], "+faststart");
    }

    #[test]
    fn progress_output_is_enabled() {
        let args = build_args(
            &TranscodeRequest {
                input: Path::new("/i.mp4"),
                output: Path::new("/o.mp4"),
                scale_to: None,
                on_progress: None,
            },
            &Encoder {
                name: "libx264",
                hardware: false,
            },
            Path::new("/t.tmp"),
        );
        let i = args
            .iter()
            .position(|a| a == "-progress")
            .expect("必须有 -progress");
        assert_eq!(args[i + 1], "pipe:1");
        assert!(args.iter().any(|a| a == "-nostats"));
    }
}
