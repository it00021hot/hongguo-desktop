//! 进程内软解直播放会话：两个壳共用，不分编码。
//!
//! 一次播放 = 一份 [`IndexedVideo`]（读 `moov` 建样本索引）+ 若干次
//! [`IndexedVideo::decode_from`]（每次跳转从目标前最近的关键帧起解）。
//! 数据由调用方用回调供给——在线流的明文 Range 或本地文件——本 crate 不碰网络与解密。
//! 仅保留 NSH 实播夹具使用的解帧能力；正式音轨视图由 hongguo-video 提供。
//!
//! 解核：
//! - HEVC（`hvc1` / `hev1`，服务端称 `h265` / `bytevc1`）→ `rusty_h265`，纯 Rust、单线程；
//! - ByteVC2（`bvc2`）→ `hongguo-bytevc2` 的 vvdec。
//!
//! 两者都输出 8-bit I420（[`DecodedFrame`]），高位深截断右移。播放路径不调用系统解码器，
//! 不编码，不启动外部进程。
//!
//! 帧的 YUV 口径（矩阵、范围）原样保留，由 [`IndexedVideo::color`] 报出；要交给换算口径
//! NSH 测试夹具使用 BGRA 转换；不再包含旧 WebView 的色彩归一或 NV12 呈现适配。

mod bgra;
mod color;
#[cfg(test)]
mod fixture_tests;
mod hevc;
mod index;
mod session;

pub use bgra::{bgra_len, copy_to_bgra};
pub use color::{FrameColor, Matrix};
pub use hongguo_bytevc2::{DecodedFrame, FramePool};
pub use index::{IndexedVideo, VideoCodec};
pub use session::{DirectSession, FrameInfo, PixelFrame, ReadAt};
