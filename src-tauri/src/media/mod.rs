//! 纯 Rust 编解码（替代 ffmpeg，零外部依赖）。
//!
//! - [`demux`]：MP4 轨道/样本解复用
//! - [`hevc`]：hvcC 参数集提取与 Annex-B 转换
//! - [`remux`]：流复制拼接（快速合并）
//! - [`codec_probe`]：各集编码是否一致（快速合并的前提）
//! - [`disk_space`]：输出卷剩余空间
//! - [`transcode`]：HEVC → H.264 重编码
//! - [`ffmpeg`]：有 ffmpeg 时的加速路径（探测 + 执行）
//! - [`capability`]：解码能力探测

pub mod capability;
pub mod codec_probe;
pub mod demux;
pub mod disk_space;
pub mod ffmpeg;
pub mod hevc;
pub mod remux;
pub mod transcode;

pub use ffmpeg::backend_info;
