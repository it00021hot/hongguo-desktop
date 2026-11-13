#![cfg(target_os = "macos")]
//! macOS 平台 HEVC 编解码与 NV12 缩放。
//! 接口语义由 `hongguo-media-platform-api` 定义，门面 `hongguo-media-platform` 按编译目标选择本 crate（反向即 Cargo 依赖环）；其余目标上空壳编译（`#![cfg]` 剥掉全部内容）。

use hongguo_media_videotoolbox as vt;

/// 平台 HEVC 转码。
pub use vt::transcode_hevc;

/// 平台编码层当前是否可用（含探测缓存）：macOS VT 会话可建即真（含 Apple 软编会话）。
pub use vt::encoder_available;

/// 平台**硬编**编码器当前是否可用（含探测缓存）。
pub use vt::hevc_hw_encoder_available;

/// 丢弃平台侧的探测缓存，下次读取时重新探测（设置页「重新检测」用）。
pub use vt::clear_probe_cache;

/// I420 编成 HEVC，编码会话由调用线程持有。
pub use vt::{YuvEncoder, with_encoder_thread};

/// 仅测试构建 / 门面 test-support：跑完整平台链路；macOS 生产入口本就是「硬编优先、软编回落」直接复用——e2e 测的就是发布行为。
#[cfg(any(test, feature = "test-support"))]
pub fn transcode_hevc_for_tests(
    req: &hongguo_media_platform_api::PlatformRequest<'_>,
) -> Option<hongguo_common::AppResult<()>> {
    transcode_hevc(req)
}
