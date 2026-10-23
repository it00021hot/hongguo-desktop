//! 纯 Rust 编解码（hongguo-media crate）。
//!
//! - [`backend`]：转码后端的统一标识（枚举与家族）
//! - [`bgra`]：4 字节像素的通道序（RGBA ⇄ BGRA，渲染上传用）
//! - [`demux`]：MP4 轨道/样本解复用
//! - [`h264`]：avcC 参数集提取（与 [`hevc`] 同构，那边喂 hvcC）
//! - [`hevc`]：hvcC 参数集提取与 Annex-B 转换
//! - [`remux`]：流复制拼接（快速合并）
//! - [`codec_probe`]：各集编码是否一致（快速合并的前提）
//! - [`disk_space`]：输出卷剩余空间
//! - [`transcode`]：HEVC → H.264 重编码
//! - [`heif`]：HEIC 封面纯 Rust 解码（封面阶梯的最后一级）
//!
//! 定位：**转码链路的纯 Rust 兜底层**（零外部依赖，不需要 ffmpeg 与平台硬编）。
//! 平台硬编（VideoToolbox / Media Foundation / WIC）、ffmpeg 执行与解码能力
//! 探测在 `crates/hongguo-media-platform`——它依赖本 crate，反向不成立。
//!
//! 本 crate 不依赖 tauri。

pub mod backend;
pub mod bgra;
pub mod codec_probe;
pub mod demux;
pub mod disk_space;
pub mod h264;
pub mod heif;
pub mod hevc;
pub mod remux;
pub mod transcode;

pub use backend::Backend;
