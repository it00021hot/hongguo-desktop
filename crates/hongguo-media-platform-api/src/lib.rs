//! 平台媒体层统一契约：平台实现与业务调用方共同依赖的**数据与接口**——视频编码枚举、码率标定、转码请求 [`PlatformRequest`]。
//! 具体实现分居 `hongguo-media-platform-macos`（VideoToolbox）与 `hongguo-media-platform-windows`（Media Foundation / WIC），由门面 `hongguo-media-platform` 按编译目标选择并 re-export。
//! 依赖方向：实现 crate 依赖本 crate，反向引用会形成 Cargo 依赖环编不过；闭集能力用具体类型静态分派不开 `dyn`（与 `hongguo_media::backend` 的枚举分发范式一致）。

pub mod transcode;
pub mod video;
pub mod yuv;

pub use transcode::PlatformRequest;
pub use video::RateProfile;
pub use yuv::{EncodedVideo, I420View};
