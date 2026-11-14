//! 平台层的统一门面：编译期按目标选择实现（macOS / Windows / Android）。[`cover`] 封面平台级静图解码（Windows WIC；其余平台返回 `None`，调用方无需写平台分支）。
//! 无状态分发：转码、能力探测、探测缓存清理（同名转发到实现 crate）；调用方只认本模块，不感知具体平台实现与原生 API。

pub mod cover;

#[cfg(target_os = "ios")]
use hongguo_media_platform_ios as imp;

#[cfg(target_os = "android")]
use hongguo_media_platform_android as imp;
#[cfg(target_os = "macos")]
use hongguo_media_platform_macos as imp;
#[cfg(target_os = "windows")]
use hongguo_media_platform_windows as imp;

use hongguo_common::AppResult;

/// 平台归档转码请求。
pub use hongguo_media_platform_api::PlatformRequest;

/// 平台编码层可用时执行 HEVC→HEVC 转码。
/// `None` 表示没有可用的归档编解码会话，`Some(Err)` 保留实际失败原因。
pub fn transcode_hevc(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if disabled_by_env() {
        return None;
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        imp::transcode_hevc(req)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = req;
        None
    }
}

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
/// 只喂能力徽标与并行度策略——macOS / Windows / Android 的生产闸门是 [`encoder_available`]，软编会话也算数。
pub fn hevc_hw_encoder_available() -> bool {
    if disabled_by_env() {
        return false;
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        imp::hevc_hw_encoder_available()
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
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
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        imp::encoder_available()
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        false
    }
}

/// 丢弃平台侧的探测缓存，下次读取时重新探测（设置页「重新检测」用）。
pub fn clear_probe_cache() {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        imp::clear_probe_cache();
    }
}

/// 测试开关：禁用平台归档编码。
pub fn disabled_by_env() -> bool {
    std::env::var_os("HONGGUO_NO_PLATFORM").is_some()
}

/// ByteVC2 解帧后的 I420 编码会话。只在有平台编码器的目标上存在。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
pub(crate) use imp::YuvEncoder;

/// 把闭包放到平台编码器要求的线程上执行。macOS / Android 就是调用线程。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
))]
pub(crate) fn imp_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    imp::with_encoder_thread(f)
}

/// 仅测试构建：跑完整平台链路（解码/Annex-B/封装/时间戳），让没有硬编的机器（含 CI）也能验证管线本身。
/// macOS / Android 生产入口直接走正式路径；Windows 分发到 `_for_tests`（强制 in-box 软编）。
/// e2e 统一从这里走，不直接引用 `vt::`/`mf::`/`mc::`（否则另一平台的测试构建编不过）；门控 `test-support`——跨 crate 后 `pub(crate)` 够不着。
#[cfg(feature = "test-support")]
pub fn transcode_hevc_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))]
    {
        imp::transcode_hevc_for_tests(req)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = req;
        None
    }
}
