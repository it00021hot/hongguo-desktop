//! 平台层的统一门面：编译期按目标选择实现（macOS / Windows / Android）。[`cover`] 封面平台级静图解码（Windows WIC；其余平台返回 `None`，调用方无需写平台分支）。
//! 无状态分发：转码、能力探测、探测缓存清理（同名转发到实现 crate）；调用方只认本模块，不感知具体平台实现与原生 API。
//! 探测纪律与 ffmpeg 侧一致：**列表里有 ≠ 能用**，必须真实建会话试编一帧（试编帧不能小于 256×256，nvenc 曾在 64×64 上误判不可用）；
//! 任何探测失败都安静返回不可用让分流链落到 ffmpeg，两者都没有时转码直接报错。

pub mod cover;

#[cfg(target_os = "android")]
use hongguo_media_platform_android as imp;
#[cfg(target_os = "macos")]
use hongguo_media_platform_macos as imp;
#[cfg(target_os = "windows")]
use hongguo_media_platform_windows as imp;

use hongguo_common::AppResult;

/// 平台硬编的一次转码请求（与 ffmpeg 侧的 [`crate::ffmpeg::TranscodeRequest`] 同构）。
pub use hongguo_media_platform_api::PlatformRequest;
/// 码率档（与 [`PlatformRequest::rate`] 同源）。
pub use hongguo_media_platform_api::RateProfile;

/// 平台编码层可用时执行 HEVC→H.264 转码。
/// 返回 `None` 表示本机平台层不接这个源（HEVC 以外、被 `HONGGUO_NO_PLATFORM` 关闭、无可用编解码会话），`Some(Err)` 表示试过但失败——两者都落回分流链的下一条路。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if disabled_by_env() {
        return None;
    }
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp::transcode_h264(req)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = req;
        None
    }
}

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
/// 只喂能力徽标与并行度策略——macOS / Windows / Android 的生产闸门是 [`encoder_available`]，软编会话也算数。
pub fn h264_hw_encoder_available() -> bool {
    if disabled_by_env() {
        return false;
    }
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp::h264_hw_encoder_available()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        false
    }
}

/// 平台编码层当前是否可用（含探测缓存）：macOS VT / Windows MF / Android MediaCodec 会话可建即真（含各端软编回落）。
/// `selected_backend` 与分流链的准入口径。
pub fn encoder_available() -> bool {
    if disabled_by_env() {
        return false;
    }
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp::encoder_available()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        false
    }
}

/// HEVC 解码是否在位：回答「在线播放能否把 HEVC 系流直接喂给 WebView」，与转码侧探测是两个问题。
/// 不吃 `HONGGUO_NO_PLATFORM`。Windows 探 MF；Android 探非软解 MediaCodec（软解只服务转码）；其余（含 macOS）恒真。
pub fn hevc_decode_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        imp::hevc_decoder_present()
    }
    #[cfg(target_os = "android")]
    {
        imp::hevc_webview_decode_available()
    }
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    {
        true
    }
}

/// 丢弃平台侧的探测缓存，下次读取时重新探测（设置页「重新检测」用）。
pub fn clear_probe_cache() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp::clear_probe_cache();
    }
}

/// 测试开关：强制按「平台硬编不可用」处理，验证回退链。
///
/// 与 `HONGGUO_NO_FFMPEG` 同一套约定。
pub fn disabled_by_env() -> bool {
    std::env::var_os("HONGGUO_NO_PLATFORM").is_some()
}

/// ByteVC2 解帧后的 I420 编码会话。只在有平台编码器的目标上存在。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
pub(crate) use imp::YuvEncoder;

/// 把闭包放到平台编码器要求的线程上执行。macOS / Android 就是调用线程。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
pub(crate) fn imp_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    imp::with_encoder_thread(f)
}

/// 仅测试构建：跑完整平台链路（解码/Annex-B/封装/时间戳），让没有硬编的机器（含 CI）也能验证管线本身。
/// macOS / Android 生产入口直接走正式路径；Windows 分发到 `_for_tests`（强制 in-box 软编）。
/// e2e 统一从这里走，不直接引用 `vt::`/`mf::`/`mc::`（否则另一平台的测试构建编不过）；门控 `test-support`——跨 crate 后 `pub(crate)` 够不着。
#[cfg(feature = "test-support")]
pub fn transcode_h264_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "android"))]
    {
        imp::transcode_h264_for_tests(req)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = req;
        None
    }
}
