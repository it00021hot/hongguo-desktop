//! 已解码的 I420 帧 → VideoToolbox H.264。ByteVC2 不经过 ffmpeg。

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::video::{RateProfile, bitrate_for_profile};
use hongguo_media_platform_api::{EncodedVideo, I420View};

use super::encode::Compressor;
use super::pixel::{Nv12Pool, Pb, Transfer};

/// 把显示序 I420 编成 H.264 Annex-B。
pub struct YuvEncoder {
    scale_to: Option<(usize, usize)>,
    fps: f64,
    rate: RateProfile,
    compressor: Option<Compressor>,
    transfer: Option<Transfer>,
    /// 源分辨率的 NV12 缓冲池（首帧建）。
    pool: Option<Nv12Pool>,
    out_dims: Option<(usize, usize)>,
    units: Vec<(f64, Vec<u8>, bool)>,
    /// 下一帧强制关键帧（首帧起播 / 重新开片段）
    force_next_keyframe: bool,
}

impl YuvEncoder {
    pub fn new(scale_to: Option<(usize, usize)>, fps: f64, rate: RateProfile) -> Self {
        Self {
            scale_to,
            fps,
            rate,
            compressor: None,
            transfer: None,
            pool: None,
            out_dims: None,
            units: Vec::new(),
            force_next_keyframe: true,
        }
    }

    pub fn push(&mut self, frame: &I420View<'_>) -> AppResult<()> {
        self.ensure_session(frame.width, frame.height)?;
        let pb = match self.pool.as_ref() {
            Some(pool) if pool.dims() == (frame.width, frame.height) => unsafe {
                pool.nv12_of(frame.y, frame.u, frame.v, frame.width, frame.height)?
            },
            // 中途变分辨率或池没建成：退回逐帧新建，行为与原来一致。
            _ => unsafe { Pb::from_i420(frame.y, frame.u, frame.v, frame.width, frame.height)? },
        };
        let frame_pb = match self.transfer.as_ref() {
            Some(transfer) => unsafe { transfer.scale(&pb)? },
            None => pb,
        };
        let compressor = self.compressor.as_mut().expect("会话刚建好");
        let force = self.force_next_keyframe;
        self.force_next_keyframe = false;
        unsafe { compressor.encode(&frame_pb, frame.pts, frame.dur, force)? };
        self.harvest()?;
        Ok(())
    }

    pub fn coded_size(&self) -> Option<(usize, usize)> {
        self.out_dims
    }

    pub fn take_completed_gop(&mut self) -> Option<(Vec<hongguo_media::mux::AnnexbFrame>, f64)> {
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
        let Some(compressor) = self.compressor.as_mut() else {
            return Err(AppError::Media("ByteVC2 没有帧可编码".into()));
        };
        unsafe { compressor.finish()? };
        self.harvest()?;
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
        if self.compressor.is_some() {
            return Ok(());
        }
        let (out_w, out_h) = match self.scale_to {
            Some((w, h)) if (w, h) != (width, height) => {
                self.transfer = Some(unsafe { Transfer::new(w, h)? });
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (width, height),
        };
        let bitrate = bitrate_for_profile(out_w, out_h, self.fps, self.rate);
        let low_latency = self.rate == RateProfile::Playback;
        self.compressor = Some(unsafe { Compressor::new(out_w, out_h, bitrate, low_latency)? });
        self.out_dims = Some((out_w, out_h));
        match unsafe { Nv12Pool::new(width, height) } {
            Ok(pool) => self.pool = Some(pool),
            Err(e) => log::warn!("[ByteVC2] NV12 缓冲池建不出来，逐帧新建缓冲: {e}"),
        }
        log::info!("[ByteVC2] VideoToolbox 编码会话 {out_w}x{out_h} low_latency={low_latency}");
        Ok(())
    }

    fn harvest(&mut self) -> AppResult<()> {
        let Some(compressor) = self.compressor.as_mut() else {
            return Ok(());
        };
        for sample in compressor.take_outputs() {
            let (annexb, keyframe) = unsafe { compressor.sample_to_unit(&sample)? };
            let pts = super::pipeline::cm_time_secs(unsafe {
                super::ffi::CMSampleBufferGetOutputPresentationTimeStamp(sample.0)
            });
            self.units.push((pts, annexb, keyframe));
        }
        Ok(())
    }
}

/// macOS 编码会话不需要专用线程。
pub fn with_encoder_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_media_platform_api::RateProfile;

    #[test]
    fn one_grey_frame_becomes_h264_annexb() {
        let mut encoder = YuvEncoder::new(None, 25.0, RateProfile::Playback);
        let y = vec![128u8; 256 * 256];
        let chroma = vec![128u8; 128 * 128];
        encoder
            .push(&I420View {
                y: &y,
                u: &chroma,
                v: &chroma,
                width: 256,
                height: 256,
                pts: 0.0,
                dur: 0.04,
            })
            .expect("送入灰帧");
        let encoded = encoder.finish().expect("收尾");
        assert_eq!((encoded.width, encoded.height), (256, 256));
        assert!(!encoded.units.is_empty(), "应有 H.264 样本");
        assert!(
            encoded.units.iter().any(|(_, data, _)| data.len() > 4),
            "样本应带载荷"
        );
    }
}
