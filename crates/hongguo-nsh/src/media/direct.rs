use super::source::Source;
use hongguo_common::{AppError, AppResult};
use hongguo_nsh_video_api::frames::{FrameInfo, FrameSource, PixelFrame};
use hongguo_softdec::{DirectSession, ReadAt};
use std::sync::{Arc, Mutex, atomic::AtomicBool};

impl ReadAt for Source {
    fn length(&self) -> AppResult<u64> {
        self.len()
            .ok_or_else(|| AppError::Media("media length is unavailable".into()))
    }

    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        *bytes = self
            .read(offset, length, cancelled)
            .map_err(AppError::Media)?;
        Ok(())
    }
}

pub(super) struct Frames {
    pub session: Arc<DirectSession>,
    current: Mutex<Option<Arc<PixelFrame>>>,
}

impl Frames {
    pub fn new(session: Arc<DirectSession>) -> Self {
        Self {
            session,
            current: Mutex::new(None),
        }
    }
}

impl FrameSource for Frames {
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
        let frame = self
            .session
            .frame_at(position_ms)
            .map_err(|error| error.to_string())?;
        let mut current = self
            .current
            .lock()
            .map_err(|_| "frame adapter lock poisoned")?;
        let Some(frame) = frame else {
            return Ok(None);
        };
        if current.as_ref().is_none_or(|current| {
            current.serial != frame.serial || current.generation != frame.generation
        }) {
            *current = Some(Arc::new(PixelFrame {
                serial: frame.serial,
                generation: frame.generation,
                pts_ms: frame.pts_ms,
                width: frame.width,
                height: frame.height,
                stride: frame.stride,
                pixels: frame.pixels.clone(),
            }));
        }
        Ok(current.clone())
    }

    fn seek(&self, position_ms: u64) -> Result<u64, String> {
        self.session
            .seek(position_ms)
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
    fn close(&self) {
        self.session.close();
    }
}
