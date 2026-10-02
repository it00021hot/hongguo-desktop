//! ffmpeg 可选加速。
//!
//! 设计取舍：**装了 ffmpeg 就用它，没装就纯 Rust 软解**。
//!
//! - 有 ffmpeg：转码交给 ffmpeg，可用 NVENC/QSV/AMF/MF 硬编码，速度接近实时；
//!   快速合并也用 `concat -c copy`，与现版行为一致。
//! - 无 ffmpeg：完全回落到 `rusty_h265` + `rusty_h264` + `muxide`，
//!   零外部依赖、纯 Rust，只是软解慢一些。
//!
//! 这样既保留了「不依赖额外软件」的特性，又让愿意装 ffmpeg 的用户拿到硬解速度，
//! 且不必为每个平台写无法验证的 VideoToolbox / Media Foundation FFI。
//!
//! 探测结果全局缓存，只查一次。

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// 常见安装位置（PATH 之外）。
#[cfg(target_os = "windows")]
const EXTRA_PATHS: &[&str] = &[
    r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    r"C:\ffmpeg\bin\ffmpeg.exe",
    r"C:\ProgramData\chocolatey\bin\ffmpeg.exe",
];

#[cfg(target_os = "macos")]
const EXTRA_PATHS: &[&str] = &[
    "/opt/homebrew/bin/ffmpeg",
    "/usr/local/bin/ffmpeg",
    "/usr/bin/ffmpeg",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const EXTRA_PATHS: &[&str] = &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

/// 硬编码器优先级：从快到慢，逐个试（现版沿用同一顺序）。
const ENCODER_PREFERENCE: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf", "h264_mf", "libx264"];

static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();
static ENCODER: OnceLock<Option<String>> = OnceLock::new();

/// 探测本机 ffmpeg 路径。找不到返回 `None`。
pub fn ffmpeg_path() -> Option<&'static PathBuf> {
    FFMPEG.get_or_init(detect).as_ref()
}

fn detect() -> Option<PathBuf> {
    // 1) PATH
    if let Some(p) = probe_command("ffmpeg") {
        return Some(p);
    }
    // 2) 常见安装位置
    for candidate in EXTRA_PATHS {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// 尝试执行 `ffmpeg -version`，成功则返回其路径。
#[cfg(target_os = "windows")]
fn probe_command(name: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new(name)
        .arg("-version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then(|| PathBuf::from(name))
}

#[cfg(not(target_os = "windows"))]
fn probe_command(name: &str) -> Option<PathBuf> {
    let out = Command::new(name).arg("-version").output().ok()?;
    out.status.success().then(|| PathBuf::from(name))
}

/// 选一个可用的 H.264 编码器。
///
/// 列表里有不等于能用（无 N 卡时 `h264_nvenc` 会失败），所以逐个**实际试编一帧**。
/// 探测一次并缓存。
pub fn h264_encoder() -> Option<&'static str> {
    let chosen = ENCODER.get_or_init(pick_encoder);
    chosen.as_deref()
}

fn pick_encoder() -> Option<String> {
    let ffmpeg = ffmpeg_path()?;

    let list = run(ffmpeg, &["-hide_banner", "-encoders"])?;
    let candidates: Vec<&str> = ENCODER_PREFERENCE
        .iter()
        .copied()
        .filter(|enc| list.contains(enc))
        .collect();
    // 列表里没有 libx264 也要试——静态构建有时不列全
    let candidates = if candidates.is_empty() {
        vec!["libx264"]
    } else {
        candidates
    };

    for enc in candidates {
        if encoder_works(ffmpeg, enc) {
            return Some(enc.to_string());
        }
        log::debug!("[FFmpeg] 编码器不可用，跳过: {enc}");
    }
    None
}

/// 实际试编一帧，验证编码器真的能用。
fn encoder_works(ffmpeg: &PathBuf, encoder: &str) -> bool {
    run(
        ffmpeg,
        &[
            "-hide_banner",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.1",
            "-frames:v",
            "1",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ],
    )
    .is_some()
}

/// 执行 ffmpeg，成功返回 stdout。
fn run(ffmpeg: &PathBuf, args: &[&str]) -> Option<String> {
    let out = Command::new(ffmpeg).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 当前解码/编码后端的可读描述，供内部日志与转码流水线使用。
///
/// 不带 ffmpeg 路径：路径由 [`ffmpeg_path`] 直接给，塞进结构体只会多一个
/// 没人读的字段。
pub struct BackendInfo {
    /// 选中的 H.264 编码器
    pub encoder: String,
    /// 实际生效的转码方式
    pub transcode_with: String,
}

/// 取当前后端信息。
pub fn backend_info() -> BackendInfo {
    match (ffmpeg_path(), h264_encoder()) {
        (Some(_), Some(enc)) => BackendInfo {
            encoder: enc.to_string(),
            transcode_with: format!("ffmpeg ({enc})"),
        },
        _ => BackendInfo {
            encoder: String::new(),
            transcode_with: "rusty_h265 → rusty_h264".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        // 探测在没装 ffmpeg 的机器上也应安全返回
        let _ = ffmpeg_path();
    }

    #[test]
    fn backend_info_has_transcode_target() {
        let info = backend_info();
        assert!(!info.transcode_with.is_empty(), "必须说明当前用什么转码");
    }

    #[test]
    fn backend_info_matches_detection() {
        let info = backend_info();
        if ffmpeg_path().is_none() {
            assert_eq!(info.encoder, "", "无 ffmpeg 时不应报告编码器");
            assert!(info.transcode_with.contains("rusty_h265"), "应回落软解");
        } else {
            assert!(!info.encoder.is_empty(), "有 ffmpeg 时应选到编码器");
        }
    }
}
