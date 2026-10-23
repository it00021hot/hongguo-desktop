//! 视频逐帧解码出口（Windows）：Media Foundation 解码 MFT（HEVC / H.264），
//! NV12 系统内存帧，渲染侧 CPU 转 RGBA 后按普通图像上传。
//!
//! 契约见 [`hongguo_media_platform_api::video`]；本类型是门面
//! `hongguo_media_platform::platform::video::VideoFrameDecoder` 在 Windows 上
//! 的具体实现。解码器接口与原 MF 侧一致（open/decode/finish，帧自带 PTS），引擎
//! （hongguo-playback）与壳（hongguo-gpui）只认这一层。

use hongguo_common::AppResult;
use hongguo_media_platform_api::video::{
    DecoderInit, FrameBuffer, Nv12Plane, VideoDecoder, VideoFrame,
};

use crate::mf::VideoMftDecoder;

/// 帧流解码器（MF 会话：open/decode/finish，帧自带 PTS）。
pub struct VideoFrameDecoder {
    inner: VideoMftDecoder,
}

impl VideoFrameDecoder {
    pub fn open(init: DecoderInit<'_>) -> AppResult<Self> {
        Ok(Self {
            inner: VideoMftDecoder::new(
                init.codec,
                init.sequence_header,
                init.parameter_sets,
                init.length_size,
                init.width,
                init.height,
            )?,
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
                dims: (f.width, f.height),
                buffer: FrameBuffer::Nv12(std::sync::Arc::new(Nv12Plane {
                    data: std::sync::Arc::new(f.data),
                    stride: f.stride,
                    width: f.width,
                    height: f.height,
                })),
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
                dims: (f.width, f.height),
                buffer: FrameBuffer::Nv12(std::sync::Arc::new(Nv12Plane {
                    data: std::sync::Arc::new(f.data),
                    stride: f.stride,
                    width: f.width,
                    height: f.height,
                })),
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
