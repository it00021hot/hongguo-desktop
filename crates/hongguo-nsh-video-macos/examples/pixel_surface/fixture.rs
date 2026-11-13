use hongguo_common::{AppError, AppResult};
use hongguo_nsh_video_api::frames::{FrameInfo, FrameSource, PixelFrame};
use hongguo_softdec::{DirectSession, ReadAt};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const MOVIE: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");

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

pub(super) struct Softdec(DirectSession);

impl Softdec {
    pub(super) fn new() -> Self {
        Self(DirectSession::open(Arc::new(Movie)).expect("synthetic HEVC fixture"))
    }
}

impl FrameSource for Softdec {
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        self.0
            .info()
            .map(|info| {
                info.map(|info| FrameInfo {
                    width: info.width,
                    height: info.height,
                })
            })
            .map_err(|error| error.to_string())
    }
    fn frame_at(&self, position: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        self.0
            .frame_at(position)
            .map(|frame| {
                frame.map(|frame| {
                    Arc::new(PixelFrame {
                        serial: frame.serial,
                        generation: frame.generation,
                        pts_ms: frame.pts_ms,
                        width: frame.width,
                        height: frame.height,
                        stride: frame.stride,
                        pixels: frame.pixels.clone(),
                    })
                })
            })
            .map_err(|error| error.to_string())
    }
    fn seek(&self, position: u64) -> Result<u64, String> {
        self.0.seek(position).map_err(|error| error.to_string())
    }
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        self.0
            .presented(generation, serial)
            .map_err(|error| error.to_string())
    }
    fn holding(&self) -> bool {
        self.0.holding()
    }
    fn close(&self) {
        self.0.close();
    }
}
