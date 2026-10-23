//! 视频逐帧解码出口（macOS）：VideoToolbox（HEVC / H.264）NV12
//! CVPixelBuffer，渲染零拷贝上屏。
//!
//! 契约见 [`hongguo_media_platform_api::video`]；本类型是门面
//! `hongguo_media_platform::platform::video::VideoFrameDecoder` 在 macOS 上的
//! 具体实现。解码器接口与原 VT 侧一致（open/decode/finish，帧自带 PTS），引擎
//! （hongguo-playback）与壳（hongguo-gpui）只认这一层。

use hongguo_common::AppResult;
use hongguo_media_platform_api::video::{DecoderInit, FrameBuffer, VideoDecoder, VideoFrame};

use crate::vt::FrameDecoder;

/// 帧流解码器（VT 会话：open/decode/finish，帧自带 PTS）。
pub struct VideoFrameDecoder {
    inner: FrameDecoder,
}

impl VideoFrameDecoder {
    pub fn open(init: DecoderInit<'_>) -> AppResult<Self> {
        Ok(Self {
            inner: FrameDecoder::open(init.codec, init.parameter_sets, init.full_range)?,
        })
    }

    /// 解一个样本（MP4 长度前缀 NAL 原样字节），返回本次产生的帧（解码序，
    /// 各自带 PTS；无输出返回空表）。
    pub fn decode(
        &mut self,
        sample: &[u8],
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<Vec<VideoFrame>> {
        let frames = self
            .inner
            .decode(sample, pts_secs, duration_secs)?
            .into_iter()
            .map(|f| VideoFrame {
                pts_secs: f.pts_secs,
                dims: f.dims(),
                buffer: FrameBuffer::CVPixelBuffer(f.buffer()),
            })
            .collect();
        Ok(frames)
    }

    /// 冲尾帧并返回遗留输出（会话随即不再可用）。
    pub fn finish(&mut self) -> AppResult<Vec<VideoFrame>> {
        let frames = self
            .inner
            .finish()?
            .into_iter()
            .map(|f| VideoFrame {
                pts_secs: f.pts_secs,
                dims: f.dims(),
                buffer: FrameBuffer::CVPixelBuffer(f.buffer()),
            })
            .collect();
        Ok(frames)
    }
}

impl VideoDecoder for VideoFrameDecoder {
    fn decode(
        &mut self,
        sample: &[u8],
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<Vec<VideoFrame>> {
        VideoFrameDecoder::decode(self, sample, pts_secs, duration_secs)
    }

    fn finish(&mut self) -> AppResult<Vec<VideoFrame>> {
        VideoFrameDecoder::finish(self)
    }
}
