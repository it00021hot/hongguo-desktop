//! H.264 MediaCodec 编码：NV12 → Annex-B。

use std::time::Duration;

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

pub struct H264Encoder {
    codec: MediaCodec,
    width: usize,
    height: usize,
    pending_config: Vec<u8>,
    pending_units: Vec<(f64, Vec<u8>, bool)>,
    ended: bool,
}

impl H264Encoder {
    pub fn new(width: usize, height: usize, fps: f64, bitrate: i32) -> AppResult<Self> {
        let codec = open_encoder()
            .ok_or_else(|| AppError::Media("本机没有 H.264 MediaCodec 编码器".into()))?;
        let mut fmt = MediaFormat::new();
        fmt.set_str("mime", "video/avc");
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
            return Err(AppError::Media("配置 H.264 编码器失败".into()));
        }
        codec
            .start()
            .map_err(|e| AppError::Media(format!("启动 H.264 编码器失败: {e:?}")))?;
        Ok(Self {
            codec,
            width,
            height,
            pending_config: Vec::new(),
            pending_units: Vec::new(),
            ended: false,
        })
    }

    pub fn push_nv12(&mut self, nv12: &[u8], pts_us: i64, eos: bool) -> AppResult<()> {
        if self.ended {
            return Ok(());
        }
        let need = self.width * self.height * 3 / 2;
        if !eos && nv12.len() < need {
            return Err(AppError::Media("编码输入 NV12 长度不足".into()));
        }
        loop {
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
        Ok(std::mem::take(&mut self.pending_units))
    }

    fn drain_output(&mut self) -> AppResult<()> {
        loop {
            match self
                .codec
                .dequeue_output_buffer(TIMEOUT)
                .map_err(|e| AppError::Media(format!("编码输出缓冲: {e:?}")))?
            {
                DequeuedOutputBufferInfoResult::TryAgainLater => break,
                DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buf) => {
                    let info = *buf.info();
                    let eos = info.flags() & EOS != 0;
                    let is_cfg = info.flags() & CODEC_CONFIG != 0;
                    let is_key = info.flags() & KEY_FRAME != 0;
                    if info.size() > 0 {
                        let view = buf.buffer();
                        let start = info.offset() as usize;
                        let end = start + info.size() as usize;
                        if end <= view.len() {
                            let chunk = view[start..end].to_vec();
                            if is_cfg {
                                self.pending_config.extend_from_slice(&chunk);
                            } else {
                                let mut annexb = Vec::new();
                                if is_key || self.pending_units.is_empty() {
                                    annexb.extend_from_slice(&self.pending_config);
                                }
                                annexb.extend_from_slice(&chunk);
                                let pts = info.presentation_time_us() as f64 / 1_000_000.0;
                                self.pending_units.push((
                                    pts,
                                    annexb,
                                    is_key || self.pending_units.is_empty(),
                                ));
                            }
                        }
                    }
                    self.codec
                        .release_output_buffer(buf, false)
                        .map_err(|e| AppError::Media(format!("释放编码输出: {e:?}")))?;
                    if eos {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for H264Encoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}

fn open_encoder() -> Option<MediaCodec> {
    for name in ["c2.android.avc.encoder", "OMX.google.h264.encoder"] {
        if let Some(c) = MediaCodec::from_codec_name(name) {
            return Some(c);
        }
    }
    MediaCodec::from_encoder_type("video/avc")
}
