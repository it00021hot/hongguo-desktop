#![cfg(target_os = "windows")]
//! Windows 平台原生实现（hongguo-media-platform-windows crate）。
//!
//! - Media Foundation（[`mf`]）：HEVC / H.264 播放解码 + HEVC 转码硬解 →
//!   NV12 → H.264 编码（硬件 MFT 优先；软编 h264_mf 仅测试通道），AAC 解码 MFT
//! - WIC（[`cover`]）：HEIC 封面平台级静图解码（HEIF/HEVC 扩展在则走系统
//!   解码器，运行时探测、可失败静默）
//!
//! 接口语义由 `hongguo-media-platform-api` 定义，门面 `hongguo-media-platform`
//! 按编译目标选择本 crate；本 crate 不依赖门面（反向即 Cargo 依赖环）。
//! 其余目标上空壳编译（`#![cfg]` 剥掉全部内容）。

pub mod audio;
pub mod audio_output;
mod cover;
mod mf;
pub mod video;

pub use audio::AacDecoder;
pub use audio_output::CpalAudioOutput;
pub use cover::heic_to_jpeg;
pub use video::VideoFrameDecoder;

/// 平台编码层可用时执行 HEVC→H.264 转码。
///
/// 仅硬件 MFT 接生产（h264_mf 软编不接，见 [`encoder_available`]）。
pub use mf::transcode_h264;

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
pub use mf::h264_hw_encoder_available;

/// HEVC 解码 MFT 是否在位（WebView2 能否解 HEVC 的判据；与转码探测同源，
/// 但不吃 `HONGGUO_NO_PLATFORM`——那个开关关的是转码平台层）。
pub use mf::hevc_decoder_present;

/// 仅测试构建 / 门面 test-support：跑完整平台链路（解码/Annex-B/封装/时间戳），
/// 让没有硬编 MFT 的机器（含 CI）也能验证管线本身（h264_mf 软编通道）。
#[cfg(any(test, feature = "test-support"))]
pub use mf::transcode_h264_for_tests;

/// 平台编码层当前是否可用（含探测缓存）。Windows：仅指有硬件 MFT。
pub fn encoder_available() -> bool {
    mf::h264_hw_encoder_available()
}

/// 丢弃平台侧的探测缓存（MF 硬编/HEVC 解码 MFT + WIC HEIF 解码器），
/// 下次读取时重新探测（设置页「重新检测」用）。
pub fn clear_probe_cache() {
    mf::clear_probe_cache();
    // WIC 的 HEIF 解码器探测缓存一并清（封面平台级，见 cover）
    cover::clear_probe_cache();
}
