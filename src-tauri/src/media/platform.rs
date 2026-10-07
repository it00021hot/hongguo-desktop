//! 平台原生硬编后端。
//!
//! - macOS：VideoToolbox（[`vt`]）——HEVC/H.264 硬解 + H.264 硬编，
//!   CVPixelBuffer 全程 NV12，编解码之间零拷贝是自然形态
//! - Windows：Media Foundation（[`mf`]）——硬件 MFT 枚举 + async 事件循环
//!
//! 探测纪律与 ffmpeg 侧一致：**列表里有 ≠ 能用**，必须真实建会话试编一帧
//! （试编帧不能小于 256×256，nvenc 曾在 64×64 上误判不可用）。
//! 任何探测失败都安静地返回不可用，让分流链落到 ffmpeg/软解。

#[cfg(target_os = "windows")]
pub mod mf;
#[cfg(target_os = "macos")]
pub mod vt;

use std::path::Path;

use crate::error::AppResult;

/// 平台硬编的一次转码请求（与 ffmpeg 侧的 [`crate::media::ffmpeg::TranscodeRequest`] 同构）。
pub struct PlatformRequest<'a> {
    /// 已解密的源 MP4
    pub input: &'a Path,
    /// 输出路径（内部先写临时文件，成功后原子替换）
    pub output: &'a Path,
    /// 统一到这个分辨率。`None` 表示保持源分辨率
    pub scale_to: Option<(u32, u32)>,
    /// 已编码秒数的回调（0.0–时长秒），用于集内进度
    pub on_progress: Option<&'a (dyn Fn(f64) + Send + Sync)>,
}

/// 平台硬编可用时执行 HEVC→H.264（含 H.264 源的直转）转码。
///
/// 返回 `None` 表示本机平台硬编不可用（或被 `HONGGUO_NO_PLATFORM` 关闭），
/// 调用方落回分流链的下一条路；`Some(Err)` 表示试过但失败，同样落回。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if disabled_by_env() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        vt::transcode_h264(req)
    }
    #[cfg(target_os = "windows")]
    {
        mf::transcode_h264(req)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = req;
        None
    }
}

/// 平台硬编 H.264 编码器当前是否可用（含探测缓存）。
pub fn h264_hw_encoder_available() -> bool {
    if disabled_by_env() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        vt::h264_hw_encoder_available()
    }
    #[cfg(target_os = "windows")]
    {
        mf::h264_hw_encoder_available()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// 丢弃平台侧的探测缓存，下次读取时重新探测（设置页「重新检测」用）。
pub fn clear_probe_cache() {
    #[cfg(target_os = "macos")]
    vt::clear_probe_cache();
    #[cfg(target_os = "windows")]
    mf::clear_probe_cache();
}

/// 测试开关：强制按「平台硬编不可用」处理，验证回退链。
///
/// 与 `HONGGUO_NO_FFMPEG` 同一套约定。
pub fn disabled_by_env() -> bool {
    std::env::var_os("HONGGUO_NO_PLATFORM").is_some()
}

/// 仅测试构建：以「允许回落软件」的策略跑完整平台链路（解码/Annex-B/封装/
/// 时间戳），让没有硬编的机器（含 CI）也能验证管线本身。
///
/// 分发到各平台的 `_for_tests` 入口——e2e 用例统一从这里走，不直接引用
/// `vt::`/`mf::`（否则另一平台的测试构建编不过）。
#[cfg(test)]
pub(crate) fn transcode_h264_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    #[cfg(target_os = "macos")]
    {
        vt::transcode_h264_for_tests(req)
    }
    #[cfg(target_os = "windows")]
    {
        mf::transcode_h264_for_tests(req)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = req;
        None
    }
}
