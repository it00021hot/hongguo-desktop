//! 平台层门面（hongguo-media-platform crate）。
//!
//! - [`platform`]：统一门面——按编译目标选择实现 crate（
//!   `hongguo-media-platform-macos` / `hongguo-media-platform-windows`），
//!   分派无状态操作（转码 / 能力探测 / 清缓存 / HEIC 封面）
//! - [`ffmpeg`]：有 ffmpeg 时的加速路径（探测 + 执行），平台无关
//! - [`capability`]：解码能力探测（平台硬解与 ffmpeg 一起探）
//!
//! 契约（`PlatformRequest` / 码率策略）在 `hongguo-media-platform-api`，
//! 平台原生实现（VideoToolbox / Media Foundation / WIC）分居两个实现
//! crate——依赖方向「实现 → 契约；门面 → 契约 +（按目标）实现」，见
//! `docs/architecture-refactor.md`。纯 Rust 的编解码（backend / demux /
//! hevc / remux / codec_probe / disk_space / transcode / heif）在
//! `crates/hongguo-media`。
//!
//! 业务侧（service / protocol / gpui）只依赖本门面，不感知具体
//! 平台实现与原生 API。转码的**分流执行**（平台 → ffmpeg → 纯 Rust 三级
//! 瀑布）在 service 的 `transcode_service::pipeline`——编排留在上层，本
//! crate 只提供每条路的能力与执行。
//!
//! 本 crate 不依赖 tauri 与 gpui。

pub mod capability;
pub mod ffmpeg;
pub mod platform;
