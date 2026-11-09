//! 已解码的 I420 帧 → Media Foundation H.264。ByteVC2 不经过 ffmpeg。

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::video::{RateProfile, bitrate_for_profile};
use hongguo_media_platform_api::{EncodedVideo, I420View};

use super::decoder::Nv12Frame;
use super::encoder::{Encoder, EncoderKind};
use super::mft::run_on_mf_thread;
use super::probe::{h264_hw_encoder_available, h264_sw_encoder_available};
use super::sample::secs_to_hns;
use super::scaler::Scaler;

/// 把显示序 I420 编成 H.264 Annex-B。必须在 MF 线程上使用。
pub struct YuvEncoder {
    scale_to: Option<(usize, usize)>,
    fps: f64,
    rate: RateProfile,
    encoder: Option<Encoder>,
    scaler: Option<Scaler>,
    out_dims: Option<(usize, usize)>,
    units: Vec<(f64, Vec<u8>, bool)>,
}

impl YuvEncoder {
    pub fn new(scale_to: Option<(usize, usize)>, fps: f64, rate: RateProfile) -> Self {
        Self {
            scale_to,
            fps,
            rate,
            encoder: None,
            scaler: None,
            out_dims: None,
            units: Vec::new(),
        }
    }

    pub fn push(&mut self, frame: &I420View<'_>) -> AppResult<()> {
        let nv12 = i420_to_nv12(frame.y, frame.u, frame.v, frame.width, frame.height)?;
        let mut nv = Nv12Frame {
            data: nv12,
            stride: frame.width,
            width: frame.width,
            height: frame.height,
            pts_100ns: secs_to_hns(frame.pts),
            dur_100ns: secs_to_hns(frame.dur),
        };
        self.ensure_session(frame.width, frame.height)?;
        if let Some(scaler) = self.scaler.as_mut() {
            nv = scaler.scale(nv)?;
        }
        let encoder = self.encoder.as_mut().expect("会话刚建好");
        encoder.encode(nv, frame.pts, frame.dur)?;
        self.units.extend(encoder.take_units()?);
        Ok(())
    }

    pub fn coded_size(&self) -> Option<(usize, usize)> {
        self.out_dims
    }

    pub fn take_completed_gop(
        &mut self,
    ) -> Option<(Vec<hongguo_media::mux::AnnexbFrame>, f64)> {
        hongguo_media::mux::split_completed_gop(&mut self.units)
    }

    pub fn take_publish_run(
        &mut self,
        count: usize,
        require_sync: bool,
    ) -> Option<(Vec<hongguo_media::mux::AnnexbFrame>, Option<f64>)> {
        hongguo_media::mux::take_run(&mut self.units, count, require_sync)
    }

    pub fn finish(mut self) -> AppResult<EncodedVideo> {
        let Some(encoder) = self.encoder.as_mut() else {
            return Err(AppError::Media("ByteVC2 没有帧可编码".into()));
        };
        encoder.finish()?;
        self.units.extend(encoder.take_units()?);
        let (w, h) = self
            .out_dims
            .ok_or_else(|| AppError::Media("ByteVC2 编码没有分辨率".into()))?;
        if self.units.is_empty() {
            return Err(AppError::Media("平台编码器没有产出 H.264".into()));
        }
        Ok(EncodedVideo {
            width: w,
            height: h,
            units: self.units,
        })
    }

    fn ensure_session(&mut self, width: usize, height: usize) -> AppResult<()> {
        if self.encoder.is_some() {
            return Ok(());
        }
        let fps_num = self.fps.round().max(1.0) as u32;
        let (out_w, out_h) = match self.scale_to {
            Some((w, h)) if (w, h) != (width, height) => {
                self.scaler = Some(Scaler::new(width, height, width, w, h, fps_num, 1)?);
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (width, height),
        };
        let kind = if h264_hw_encoder_available() {
            EncoderKind::Hardware
        } else if h264_sw_encoder_available() {
            EncoderKind::Software
        } else {
            return Err(AppError::Media(
                "没有可用的 H.264 编码器，ByteVC2 不能改走 ffmpeg 解码".into(),
            ));
        };
        let bitrate = bitrate_for_profile(out_w, out_h, self.fps, self.rate);
        self.encoder = Some(Encoder::new(out_w, out_h, fps_num, 1, bitrate, kind)?);
        self.out_dims = Some((out_w, out_h));
        log::info!("[ByteVC2] Media Foundation 编码会话 {out_w}x{out_h}");
        Ok(())
    }
}

fn i420_to_nv12(y: &[u8], u: &[u8], v: &[u8], width: usize, height: usize) -> AppResult<Vec<u8>> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(AppError::Media(format!("帧尺寸无法编码: {width}x{height}")));
    }
    let chroma = width / 2 * height / 2;
    if y.len() < width * height || u.len() < chroma || v.len() < chroma {
        return Err(AppError::Media("I420 平面长度不足".into()));
    }
    let mut nv12 = Vec::with_capacity(width * height * 3 / 2);
    nv12.extend_from_slice(&y[..width * height]);
    let cw = width / 2;
    let ch = height / 2;
    for row in 0..ch {
        for col in 0..cw {
            nv12.push(u[row * cw + col]);
            nv12.push(v[row * cw + col]);
        }
    }
    Ok(nv12)
}

/// MF 编码器必须留在已 `MFStartup` 的线程上。
pub fn with_encoder_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    run_on_mf_thread(f)
}
