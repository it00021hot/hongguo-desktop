#![cfg(target_os = "windows")]
//! Windows 平台原生实现：Media Foundation（[`mf`]）——HEVC 解码 → NV12 → HEVC 编码；
//! WIC（[`cover`]）——HEIC 封面平台级静图解码（HEIF/HEVC 扩展在则走系统解码器，运行时探测、可失败静默）。
//! 接口语义由 `hongguo-media-platform-api` 定义，门面 `hongguo-media-platform` 按编译目标选择本 crate（反向即 Cargo 依赖环）；其余目标上空壳编译（`#![cfg]` 剥掉全部内容）。

mod cover;
mod mf;

pub use cover::heic_to_jpeg;

/// 平台编码层可用时执行 HEVC 重编码。
pub use mf::transcode_hevc;

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
pub use mf::hevc_hw_encoder_available;

/// 仅测试构建 / 门面 test-support：强制软件编码器跑完整平台链路。
#[cfg(any(test, feature = "test-support"))]
pub use mf::transcode_hevc_for_tests;

/// 平台编码层当前是否可用（含探测缓存）：硬编或 in-box 软编可建会话即真。
pub fn encoder_available() -> bool {
    mf::hevc_hw_encoder_available() || mf::hevc_sw_encoder_available()
}

/// ByteVC2 解出的 I420 编成 HEVC。会话建在 Media Foundation 线程上。
pub use mf::{YuvEncoder, with_encoder_thread};

/// 丢弃平台侧的探测缓存（MF 硬编/HEVC 解码 MFT + WIC HEIF 解码器），
/// 下次读取时重新探测（设置页「重新检测」用）。
pub fn clear_probe_cache() {
    mf::clear_probe_cache();
    // WIC 的 HEIF 解码器探测缓存一并清（封面平台级，见 cover）
    cover::clear_probe_cache();
}
