//! 纯 Rust 编解码（替代 ffmpeg，零外部依赖）。
//!
//! - [`demux`]：MP4 轨道/样本解复用
//! - [`hevc`]：hvcC 参数集提取与 Annex-B 转换
//! - [`remux`]：流复制拼接（快速合并）
//! - [`transcode`]：HEVC → H.264 重编码
//! - [`ffmpeg`]：有 ffmpeg 时的加速路径（探测 + 执行）
//! - [`capability`]：解码能力探测

pub mod capability;
pub mod demux;
pub mod ffmpeg;
pub mod hevc;
pub mod remux;
pub mod transcode;

pub use ffmpeg::backend_info;
