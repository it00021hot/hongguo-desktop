//! HEVC MediaCodec 解码：MP4 长度前缀样本 → Annex-B → NV12 帧。

use std::time::Duration;

use hongguo_common::{AppError, AppResult};
use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

use super::yuv::pack_nv12_from_plane;

const TIMEOUT: Duration = Duration::from_millis(10);
const EOS: u32 = 4;

pub struct HevcDecoder {
    codec: MediaCodec,
    width: usize,
    height: usize,
    stride: usize,
    slice_height: usize,
    ended: bool,
}

pub struct Nv12Frame {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub pts_us: i64,
}

impl HevcDecoder {
    pub fn new(width: usize, height: usize, csd: &[u8]) -> AppResult<Self> {
        let codec = MediaCodec::from_decoder_type("video/hevc")
            .ok_or_else(|| AppError::Media("本机没有 HEVC MediaCodec 解码器".into()))?;
        let mut fmt = MediaFormat::new();
        fmt.set_str("mime", "video/hevc");
        fmt.set_i32("width", width as i32);
        fmt.set_i32("height", height as i32);
        fmt.set_buffer("csd-0", csd);
        codec
            .configure(&fmt, None, MediaCodecDirection::Decoder)
            .map_err(|e| AppError::Media(format!("配置 HEVC 解码器失败: {e:?}")))?;
        codec
            .start()
            .map_err(|e| AppError::Media(format!("启动 HEVC 解码器失败: {e:?}")))?;
        Ok(Self {
            codec,
            width,
            height,
            stride: width,
            slice_height: height,
            ended: false,
        })
    }

    pub fn push(&mut self, annexb: &[u8], pts_us: i64, eos: bool) -> AppResult<()> {
        if self.ended {
            return Ok(());
        }
        loop {
            match self
                .codec
                .dequeue_input_buffer(TIMEOUT)
                .map_err(|e| AppError::Media(format!("解码输入缓冲: {e:?}")))?
            {
                DequeuedInputBufferResult::TryAgainLater => {
                    let _ = self.pull()?;
                }
                DequeuedInputBufferResult::Buffer(mut buf) => {
                    let view = buf.buffer_mut();
                    if view.len() < annexb.len() {
                        return Err(AppError::Media("解码输入缓冲太小".into()));
                    }
                    for (dst, src) in view.iter_mut().zip(annexb.iter()) {
                        dst.write(*src);
                    }
                    let flags = if eos { EOS } else { 0 };
                    self.codec
                        .queue_input_buffer(buf, 0, annexb.len(), pts_us as u64, flags)
                        .map_err(|e| AppError::Media(format!("排队解码输入: {e:?}")))?;
                    if eos {
                        self.ended = true;
                    }
                    return Ok(());
                }
            }
        }
    }

    pub fn pull(&mut self) -> AppResult<Vec<Nv12Frame>> {
        let mut out = Vec::new();
        loop {
            match self
                .codec
                .dequeue_output_buffer(TIMEOUT)
                .map_err(|e| AppError::Media(format!("解码输出缓冲: {e:?}")))?
            {
                DequeuedOutputBufferInfoResult::TryAgainLater => break,
                DequeuedOutputBufferInfoResult::OutputFormatChanged => {
                    self.refresh_format();
                }
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buf) => {
                    let info = *buf.info();
                    let eos = info.flags() & EOS != 0;
                    if info.size() > 0 {
                        let view = buf.buffer();
                        let start = info.offset() as usize;
                        let end = start + info.size() as usize;
                        if end <= view.len() {
                            let packed = pack_nv12_from_plane(
                                &view[start..end],
                                self.width,
                                self.height,
                                self.stride,
                                self.slice_height,
                            )?;
                            out.push(Nv12Frame {
                                data: packed,
                                width: self.width,
                                height: self.height,
                                pts_us: info.presentation_time_us(),
                            });
                        }
                    }
                    self.codec
                        .release_output_buffer(buf, false)
                        .map_err(|e| AppError::Media(format!("释放解码输出: {e:?}")))?;
                    if eos {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    fn refresh_format(&mut self) {
        let fmt = self.codec.output_format();
        if let Some(w) = fmt.i32("width") {
            self.width = w.max(0) as usize;
        }
        if let Some(h) = fmt.i32("height") {
            self.height = h.max(0) as usize;
        }
        self.stride = fmt
            .i32("stride")
            .map(|v| v.max(0) as usize)
            .unwrap_or(self.width)
            .max(self.width);
        self.slice_height = fmt
            .i32("slice-height")
            .map(|v| v.max(0) as usize)
            .unwrap_or(self.height)
            .max(self.height);
        if let (Some(l), Some(r)) = (fmt.i32("crop-left"), fmt.i32("crop-right")) {
            self.width = (r - l + 1).max(0) as usize;
        }
        if let (Some(t), Some(b)) = (fmt.i32("crop-top"), fmt.i32("crop-bottom")) {
            self.height = (b - t + 1).max(0) as usize;
        }
    }
}

impl Drop for HevcDecoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}
