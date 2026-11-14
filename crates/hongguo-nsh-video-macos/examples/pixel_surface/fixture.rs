use super::bgra::{bgra_len, copy_to_bgra};
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{ChromaSampling, FrameImage};
use hongguo_nsh_video_api::frames::{FrameInfo, FrameSource, PixelFrame};
use hongguo_playback::{PlaybackSession, ReadAt, SessionConfig};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const MOVIE: &[u8] =
    include_bytes!("../../../hongguo-media/tests/fixtures/hevc-main-136x76-aac.mp4");

struct Movie;

impl ReadAt for Movie {
    fn length(&self) -> AppResult<u64> {
        Ok(MOVIE.len() as u64)
    }

    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        let start = usize::try_from(offset)
            .map_err(|_| AppError::Media("fixture offset exceeds usize".into()))?;
        let end = offset
            .checked_add(length)
            .and_then(|end| usize::try_from(end).ok())
            .ok_or_else(|| AppError::Media("fixture range overflow".into()))?;
        let data = MOVIE
            .get(start..end)
            .ok_or_else(|| AppError::Media("fixture range out of bounds".into()))?;
        bytes.clear();
        bytes.extend_from_slice(data);
        Ok(())
    }
}

/// 合成 HEVC 夹具：`hongguo-playback` 软解会话 + 示例内换色，产像素回归用的 BGRA 帧。
pub(super) struct Softdec {
    session: Arc<PlaybackSession>,
}

impl Softdec {
    pub(super) fn new() -> Self {
        let session = PlaybackSession::open(Arc::new(Movie), SessionConfig::default())
            .expect("synthetic HEVC fixture");
        Self {
            session: Arc::new(session),
        }
    }

    /// 与夹具共用手动构造的时钟（音轨时钟），让解码节奏跟随暂停/恢复。
    pub(super) fn attach_clock(
        &self,
        clock: Arc<dyn hongguo_native_clock::Clock>,
    ) -> Result<(), String> {
        self.session
            .configure_clock(clock)
            .map_err(|error| error.to_string())
    }
}

impl super::clocked::Paced for Softdec {
    fn attach_clock(&self, clock: Arc<dyn hongguo_native_clock::Clock>) {
        if let Err(error) = Self::attach_clock(self, clock) {
            log::warn!("PIXEL_FIXTURE clock attach failed: {error}");
        }
    }
}

    fn pixel(&self, frame: &hongguo_media_types::PresentationFrame) -> Option<Arc<PixelFrame>> {
        let FrameImage::Cpu(native) = &frame.image else {
            return None;
        };
        if native.bit_depth != 8 || native.chroma != ChromaSampling::Yuv420 {
            return None;
        }
        let [luma, cb, cr] = native.planes.as_slice() else {
            return None;
        };
        let (width, height) = (native.width, native.height);
        let stride = width * 4;
        let mut out = vec![0u8; bgra_len(width, height, stride)];
        let color = super::bgra::FrameColor::from_metadata(
            native.color.matrix,
            native.color.full_range,
            height,
        );
        if !copy_to_bgra(
            width,
            height,
            &luma.data,
            &cb.data,
            &cr.data,
            color,
            &mut out,
            stride,
        ) {
            return None;
        }
        Some(Arc::new(PixelFrame {
            serial: frame.serial,
            generation: frame.generation,
            pts_ms: (frame.pts_secs.max(0.0) * 1000.0).round() as u64,
            width: width as u32,
            height: height as u32,
            stride: stride as u32,
            pixels: Arc::from(out),
        }))
    }
}

impl FrameSource for Softdec {
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        self.session
            .info()
            .map(|info| {
                info.map(|info| FrameInfo {
                    width: info.width,
                    height: info.height,
                })
            })
            .map_err(|error| error.to_string())
    }
    fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        let position = f64::from(position_ms) / 1000.0;
        let frame = self
            .session
            .frame_at(position)
            .map_err(|error| error.to_string())?;
        match frame.as_deref().and_then(|frame| self.pixel(frame)) {
            Some(pixel) => Ok(Some(pixel)),
            None => Ok(None),
        }
    }
    fn seek(&self, position_ms: u64) -> Result<u64, String> {
        self.session
            .seek(f64::from(position_ms) / 1000.0)
            .map_err(|error| error.to_string())
    }
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        self.session
            .presented(generation, serial)
            .map_err(|error| error.to_string())
    }
    fn holding(&self) -> bool {
        self.session.holding()
    }
    fn control(
        &self,
        control: hongguo_nsh_video_api::frames::PlaybackControl,
    ) -> Result<(), String> {
        self.session
            .set_volume(control.volume, control.muted)
            .map_err(|error| error.to_string())?;
        self.session
            .set_rate(control.rate)
            .map_err(|error| error.to_string())?;
        self.session
            .set_playing(control.playing)
            .map_err(|error| error.to_string())
    }
    fn close(&self) {
        self.session.close();
    }
}
