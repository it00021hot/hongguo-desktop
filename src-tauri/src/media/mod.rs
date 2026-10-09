//! 纯 Rust 编解码（替代 ffmpeg，零外部依赖）。
//!
//! - [`backend`]：转码后端的统一标识（枚举与家族）
//! - [`demux`]：MP4 轨道/样本解复用
//! - [`hevc`]：hvcC 参数集提取与 Annex-B 转换
//! - [`remux`]：流复制拼接（快速合并）
//! - [`codec_probe`]：各集编码是否一致（快速合并的前提）
//! - [`disk_space`]：输出卷剩余空间
//! - [`transcode`]：HEVC → H.264 重编码
//! - [`ffmpeg`]：有 ffmpeg 时的加速路径（探测 + 执行）
//! - [`platform`]：平台原生硬编（VideoToolbox / Media Foundation）
//! - [`capability`]：解码能力探测
//! - [`heif`]：HEIC 封面纯 Rust 解码（封面阶梯的最后一级）

pub mod backend;
pub mod capability;
pub mod codec_probe;
pub mod demux;
pub mod disk_space;
pub mod ffmpeg;
pub mod heif;
pub mod hevc;
pub mod platform;
pub mod remux;
pub mod transcode;

pub use backend::Backend;
