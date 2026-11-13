//! 平台层门面：[`platform`]（按编译目标选择实现 crate，分派转码 / 能力探测 / 清缓存 / HEIC 封面）、[`ffmpeg`]（有 ffmpeg 时的加速路径，平台无关）、[`capability`]（解码能力探测）。
//! 契约（`PlatformRequest` / 码率策略）在 `hongguo-media-platform-api`，原生实现（VideoToolbox / Media Foundation / WIC）分居两个实现 crate——依赖方向「实现 → 契约；门面 → 契约 +（按目标）实现」；纯 Rust 编解码在 `crates/hongguo-media`。
//! 业务侧（service / protocol / gpui）只依赖本门面，不感知具体平台实现；转码的**三级瀑布分流**（平台 → ffmpeg → 纯 Rust）在 service 的 `transcode_service::pipeline`，本 crate 只提供每条路的能力与执行。不依赖 tauri 与 gpui。

pub mod bytevc2_transcode;
pub mod capability;
pub mod ffmpeg;
pub mod platform;

pub use bytevc2_transcode::transcode_bytevc2;
