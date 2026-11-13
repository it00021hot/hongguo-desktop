//! 进程内软解直播放会话：两个壳共用，不分编码。
//!
//! 一次播放 = 一份 [`IndexedVideo`]（读 `moov` 建样本索引）+ 若干次
//! [`IndexedVideo::decode_from`]（每次跳转从目标前最近的关键帧起解）。
//! 数据由调用方用回调供给——在线流的明文 Range 或本地文件——本 crate 不碰网络与解密。
//! 音频不在这里解：[`IndexedVideo::mask_video_tracks`] 给出「只剩音轨」的原 MP4 视图，
//! 交给壳里现成的系统媒体能力播放并充当时钟。
//!
//! 解核：
//! - HEVC（`hvc1` / `hev1`，服务端称 `h265` / `bytevc1`）→ `rusty_h265`，纯 Rust、单线程；
//! - ByteVC2（`bvc2`）→ `hongguo-bytevc2` 的 vvdec。
//!
//! 两者都输出 8-bit I420（[`DecodedFrame`]），高位深截断右移。播放路径不调用系统解码器，
//! 不编码，不启动外部进程。
//!
//! 帧的 YUV 口径（矩阵、范围）原样保留，由 [`IndexedVideo::color`] 报出；要交给换算口径
//! 不可控的显示端时，用 [`FrameColor::convert_to_bt601_full`] 归一。

mod bgra;
mod color;
#[cfg(test)]
mod fixture_tests;
mod hevc;
mod index;
mod nv12;
mod session;

pub use bgra::{bgra_len, copy_to_bgra};
pub use color::{FrameColor, Matrix};
pub use hongguo_bytevc2::{DecodedFrame, FramePool};
pub use index::{IndexedVideo, VideoCodec};
pub use nv12::{Nv12Planes, copy_to_nv12};
pub use session::{DirectSession, FrameInfo, PixelFrame, ReadAt};
