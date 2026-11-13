#![cfg(target_os = "android")]
//! Android 平台 HEVC 解码与编码。
//! 接口语义由 `hongguo-media-platform-api` 定义，门面 `hongguo-media-platform` 按编译目标选择本 crate；
//! 其余目标上空壳编译（`#![cfg]` 剥掉全部内容）。平台依赖（ndk）只在本 crate 的 target 表。

// 显式拉 mediandk：ndk 的 media feature 在部分 LTO 组合下不会把 NEEDED 留到最终 .so。
#[link(name = "mediandk")]
unsafe extern "C" {}

/// 链接期保活标记：确认 Android MediaCodec 实现进了最终 so。
#[used]
#[unsafe(no_mangle)]
pub static HONGGUO_ANDROID_MC_MARK: [u8; 24] = *b"HONGGUO_ANDROID_MC_V1\0\0\0";

mod decode;
mod encode;
mod encoded_output;
mod pipeline;
mod probe;
mod yuv;
mod yuv_in;

pub use pipeline::transcode_hevc;

/// HEVC 灰帧试编成功才可用。
pub use probe::encoder_available;

/// 平台**硬编**是否可用（徽标/并行度）；Android 上以非 google/c2.android 软编名为硬编近似。
pub use probe::hevc_hw_encoder_available;

/// 转码侧：任意 HEVC 解码器（含软解）可建即真。
pub use probe::hevc_decoder_present;

/// 丢弃探测缓存。
pub use probe::clear_probe_cache;

/// I420 编成 HEVC。
pub use yuv_in::{YuvEncoder, with_encoder_thread};

/// 测试通道：与生产同路径（设备上无法强制「只要软编」时与正式入口一致）。
#[cfg(any(test, feature = "test-support"))]
pub fn transcode_hevc_for_tests(
    req: &hongguo_media_platform_api::PlatformRequest<'_>,
) -> Option<hongguo_common::AppResult<()>> {
    transcode_hevc(req)
}
