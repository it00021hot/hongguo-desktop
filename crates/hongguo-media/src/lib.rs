//! 纯 Rust 编解码：**媒体层的公共底座**——容器解析（demux）、MP4 写出（mp4）、HEVC 重封装（mux）、
//! 流复制拼接（remux）、编码一致探测（codec_probe）。HEVC 容器辅助解析由 `hongguo-video` 提供。
//! 归档编码由原生平台编码器完成，
//! 聚合在 `crates/hongguo-media-platform`——它依赖本 crate，反向不成立。

pub mod backend;
pub mod codec_probe;
pub mod demux;
pub mod disk_space;
pub mod mp4;
pub mod mux;
pub mod remux;
pub mod transcode;

pub use backend::Backend;
