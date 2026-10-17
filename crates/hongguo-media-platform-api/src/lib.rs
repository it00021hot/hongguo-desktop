//! 平台媒体层统一契约（hongguo-media-platform-api crate）。
//!
//! 这里定义平台实现与业务调用方共同依赖的**数据与接口**：视频帧载体、
//! 有状态解码器的生命周期 trait（[`video::VideoDecoder`] /
//! [`audio::AudioDecoder`]）、转码请求 [`PlatformRequest`] 与共享码率策略。
//!
//! 具体实现分居 `hongguo-media-platform-macos`（VideoToolbox / AudioToolbox）
//! 与 `hongguo-media-platform-windows`（Media Foundation / WIC），由门面
//! `hongguo-media-platform` 按编译目标选择并 re-export（见
//! `docs/architecture-refactor.md`）。
//!
//! 依赖方向：实现 crate 依赖本 crate，本 crate 不依赖任何实现；反向引用会
//! 形成 Cargo 依赖环，编不过。闭集能力用具体类型静态分派，不开 `dyn`——
//! 与仓库既有范式（`hongguo_media::backend` 的枚举分发）一致。

pub mod audio;
pub mod transcode;
pub mod video;

pub use audio::AudioDecoder;
pub use transcode::PlatformRequest;
#[cfg(target_os = "windows")]
pub use video::Nv12Plane;
pub use video::{DecoderInit, FrameBuffer, VideoCodec, VideoDecoder, VideoFrame};
