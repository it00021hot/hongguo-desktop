#![cfg(target_os = "android")]
//! Android 平台原生实现：MediaCodec——HEVC 解码 → YUV → H.264 编码（系统软/硬由设备决定；模拟器通常软编）。
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
mod pipeline;
mod probe;
mod yuv;

/// 平台编码层可用时执行 HEVC→H.264 转码。
pub use pipeline::transcode_h264;

/// 平台编码层当前是否可用（含探测缓存）：能建 H.264 编码会话即真。
pub use probe::encoder_available;

/// 平台**硬编**是否可用（徽标/并行度）；Android 上以非 google/c2.android 软编名为硬编近似。
pub use probe::h264_hw_encoder_available;

/// 转码侧：任意 HEVC 解码器（含软解）可建即真。
pub use probe::hevc_decoder_present;

/// WebView 直解是否可信（排除 google / c2.android 软解）。
pub use probe::hevc_webview_decode_available;

/// 丢弃探测缓存。
pub use probe::clear_probe_cache;

/// 测试通道：与生产同路径（设备上无法强制「只要软编」时与正式入口一致）。
#[cfg(any(test, feature = "test-support"))]
pub fn transcode_h264_for_tests(
    req: &hongguo_media_platform_api::PlatformRequest<'_>,
) -> Option<hongguo_common::AppResult<()>> {
    transcode_h264(req)
}
