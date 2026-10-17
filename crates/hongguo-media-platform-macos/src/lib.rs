#![cfg(target_os = "macos")]
//! macOS 平台原生实现（hongguo-media-platform-macos crate）。
//!
//! - VideoToolbox（[`vt`]）：HEVC / H.264 硬解 + H.264 编码（硬编优先，
//!   无硬编回落 Apple 内置软编会话），CVPixelBuffer 全程 NV12，
//!   编解码之间零拷贝
//! - AudioToolbox（[`audio`]）：AAC 解码（AudioConverter，mpv 的 coreaudio
//!   AAC 解码同款）
//!
//! 接口语义由 `hongguo-media-platform-api` 定义，门面 `hongguo-media-platform`
//! 按编译目标选择本 crate；本 crate 不依赖门面（反向即 Cargo 依赖环）。
//! 其余目标上空壳编译（`#![cfg]` 剥掉全部内容）。

pub mod audio;
pub mod video;
mod vt;

pub use audio::AacDecoder;
pub use video::VideoFrameDecoder;

/// 平台编码层可用时执行 HEVC→H.264 转码（硬编优先，软编回落）。
pub use vt::transcode_h264;

/// 平台编码层当前是否可用（含探测缓存）。macOS：VT 会话可建即真
/// （含 Apple 软编会话）。
pub use vt::encoder_available;

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
pub use vt::h264_hw_encoder_available;

/// 丢弃平台侧的探测缓存，下次读取时重新探测（设置页「重新检测」用）。
pub use vt::clear_probe_cache;

/// 仅测试构建 / 门面 test-support：跑完整平台链路（解码/Annex-B/封装/时间戳），
/// 让没有硬编的机器（含 CI）也能验证管线本身。
///
/// macOS 的生产入口已经就是「硬编优先、软编回落」，直接复用——
/// e2e 测的就是发布行为。
#[cfg(any(test, feature = "test-support"))]
pub fn transcode_h264_for_tests(
    req: &hongguo_media_platform_api::PlatformRequest<'_>,
) -> Option<hongguo_common::AppResult<()>> {
    transcode_h264(req)
}
