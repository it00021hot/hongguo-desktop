//! 纯 Rust 编解码：**媒体层的公共底座**——容器解析（demux）、MP4 写出（mp4）、H.264 重封装（mux）、
//! 流复制拼接（remux）、avcC/hvcC 参数集提取、HEIC 封面纯 Rust 解码（heif）、编码一致探测（codec_probe）。
//! 转码的编码环节在平台硬编（VideoToolbox / Media Foundation）与 ffmpeg 两条路上，
//! 聚合在 `crates/hongguo-media-platform`——它依赖本 crate，反向不成立；本 crate 不依赖 tauri。

pub mod backend;
pub mod bgra;
pub mod codec_probe;
pub mod demux;
pub mod disk_space;
pub mod h264;
pub mod heif;
pub mod hevc;
pub mod mp4;
pub mod mux;
pub mod remux;
pub mod transcode;

pub use backend::Backend;
