//! HEVC MediaCodec 编码：NV12 → Annex-B。

use std::time::{Duration, Instant};

use hongguo_common::{AppError, AppResult};
use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

/// COLOR_FormatYUV420SemiPlanar（NV12）。
pub const COLOR_YUV420_SEMIPLANAR: i32 = 21;

const TIMEOUT: Duration = Duration::from_millis(10);
const EOS: u32 = 4;
const KEY_FRAME: u32 = 1;
const CODEC_CONFIG: u32 = 2;
const ENCODER_WAIT: Duration = Duration::from_secs(30);
pub(super) const HEVC_MIME: &str = "video/hevc";

pub struct HevcEncoder {
    codec: MediaCodec,
    width: usize,
    height: usize,
    output: super::encoded_output::EncodedOutput,
    ended: bool,
    output_ended: bool,
}

impl HevcEncoder {
    pub fn new(width: usize, height: usize, fps: f64, bitrate: i32) -> AppResult<Self> {
        let codec = MediaCodec::from_encoder_type(HEVC_MIME)
            .ok_or_else(|| AppError::Media("本机没有 HEVC MediaCodec 编码器".into()))?;
        let mut fmt = MediaFormat::new();
        fmt.set_str("mime", HEVC_MIME);
        fmt.set_i32("width", width as i32);
        fmt.set_i32("height", height as i32);
        fmt.set_i32("bitrate", bitrate.max(1));
        fmt.set_i32("frame-rate", fps.round().max(1.0) as i32);
        fmt.set_i32("i-frame-interval", 2);
        // SemiPlanar（0x15）在 MuMu 软编上可配；Flexible 常 ErrorUnknown。
        const COLOR_YUV420_FLEXIBLE: i32 = 0x7f_42_05_00;
        let mut configured = false;
        for color in [COLOR_YUV420_SEMIPLANAR, COLOR_YUV420_FLEXIBLE] {
            fmt.set_i32("color-format", color);
            if codec
                .configure(&fmt, None, MediaCodecDirection::Encoder)
                .is_ok()
            {
                configured = true;
                break;
            }
        }
        if !configured {
            return Err(AppError::Media("配置 HEVC 编码器失败".into()));
        }
        codec
            .start()
            .map_err(|e| AppError::Media(format!("启动 HEVC 编码器失败: {e:?}")))?;
        Ok(Self {
            codec,
            width,
            height,
            output: super::encoded_output::EncodedOutput::default(),
            ended: false,
            output_ended: false,
        })
    }

    pub fn push_nv12(&mut self, nv12: &[u8], pts_us: i64, eos: bool) -> AppResult<()> {
        if self.ended {
            return Ok(());
        }
        let need = self.width * self.height * 3 / 2;
        let started = Instant::now();
        if !eos && nv12.len() < need {
            return Err(AppError::Media("编码输入 NV12 长度不足".into()));
        }
        loop {
            if started.elapsed() > ENCODER_WAIT {
                return Err(AppError::Media("HEVC 编码输入等待超时".into()));
            }
            self.drain_output()?;
            match self
                .codec
                .dequeue_input_buffer(TIMEOUT)
                .map_err(|e| AppError::Media(format!("编码输入缓冲: {e:?}")))?
            {
                DequeuedInputBufferResult::TryAgainLater => continue,
                DequeuedInputBufferResult::Buffer(mut buf) => {
                    let view = buf.buffer_mut();
                    let n = if eos && nv12.is_empty() {
                        0
                    } else {
                        if view.len() < need {
                            return Err(AppError::Media("编码输入缓冲太小".into()));
                        }
                        for (dst, src) in view.iter_mut().zip(nv12.iter().take(need)) {
                            dst.write(*src);
                        }
                        need
                    };
                    let flags = if eos { EOS } else { 0 };
                    self.codec
                        .queue_input_buffer(buf, 0, n, pts_us as u64, flags)
                        .map_err(|e| AppError::Media(format!("排队编码输入: {e:?}")))?;
                    if eos {
                        self.ended = true;
                    }
                    return Ok(());
                }
            }
        }
    }

    /// 收割 `(pts_secs, annexb, keyframe)`。
    pub fn pull(&mut self) -> AppResult<Vec<(f64, Vec<u8>, bool)>> {
        self.drain_output()?;
        Ok(self.output.take())
    }

    pub fn using_hardware(&self) -> AppResult<bool> {
        let name = self
            .codec
            .name()
            .map_err(|error| AppError::Media(format!("读取 HEVC 编码器名称: {error:?}")))?;
        Ok(!(name.starts_with("c2.android.") || name.starts_with("OMX.google.")))
    }

    fn drain_output(&mut self) -> AppResult<()> {
        let started = Instant::now();
        loop {
            if self.output_ended {
                break;
            }
            match self
                .codec
                .dequeue_output_buffer(TIMEOUT)
                .map_err(|e| AppError::Media(format!("编码输出缓冲: {e:?}")))?
            {
                DequeuedOutputBufferInfoResult::TryAgainLater => {
                    if !self.ended {
                        break;
                    }
                    if started.elapsed() > ENCODER_WAIT {
                        return Err(AppError::Media("HEVC 编码收尾等待超时".into()));
                    }
                }
                DequeuedOutputBufferInfoResult::OutputFormatChanged => {
                    let format = self.codec.output_format();
                    if let Some(csd) = format.buffer("csd-0") {
                        self.output.configure(csd)?;
                    }
                }
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buf) => {
                    let info = *buf.info();
                    let eos = info.flags() & EOS != 0;
                    let is_cfg = info.flags() & CODEC_CONFIG != 0;
                    let is_key = info.flags() & KEY_FRAME != 0;
                    let result = if info.size() > 0 {
                        let view = buf.buffer();
                        let start = info.offset() as usize;
                        let end = start + info.size() as usize;
                        if end <= view.len() {
                            let chunk = view[start..end].to_vec();
                            if is_cfg {
                                self.output.configure(&chunk)
                            } else {
                                let pts = info.presentation_time_us() as f64 / 1_000_000.0;
                                self.output.frame(pts, &chunk, is_key)
                            }
                        } else {
                            Err(AppError::Media("HEVC 编码输出越出缓冲区".into()))
                        }
                    } else {
                        Ok(())
                    };
                    self.codec
                        .release_output_buffer(buf, false)
                        .map_err(|e| AppError::Media(format!("释放编码输出: {e:?}")))?;
                    result?;
                    if eos {
                        self.output_ended = true;
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for HevcEncoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}
