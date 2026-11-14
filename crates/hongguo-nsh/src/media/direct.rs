use super::source::Source;
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{DecodeDevice, PresentationFrame};
use hongguo_nsh_video_api::frames::{
    FrameInfo, FrameSource, PixelFrame, PlaybackControl, PlaybackPosition,
};
use hongguo_playback::{PlaybackSession, ReadAt};
use std::sync::{Arc, atomic::AtomicBool};

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
    pub session: Arc<PlaybackSession>,
}

impl Frames {
    pub fn new(session: Arc<PlaybackSession>) -> Self {
        Self { session }
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

    fn frame_at(&self, _position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        Ok(None)
    }

    fn native_frame_at(
        &self,
        position_secs: f64,
    ) -> Result<Option<Arc<PresentationFrame>>, String> {
        self.session
            .frame_at(position_secs)
            .map_err(|error| error.to_string())
    }

    fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> Result<(), String> {
        self.session
            .configure_decoder(device)
            .map_err(|error| error.to_string())
    }

    fn reject_decoder(&self, detail: &str) -> Result<(), String> {
        self.session
            .reject_decoder(detail.to_owned())
            .map_err(|error| error.to_string())
    }

    fn recover_presentation(&self, detail: &str) -> Result<(), String> {
        self.session
            .recover_presentation(detail.to_owned())
            .map_err(|error| error.to_string())
    }

    fn control(&self, control: PlaybackControl) -> Result<(), String> {
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

    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        self.session
            .position()
            .map(|position| {
                Some(PlaybackPosition {
                    position_secs: position.position_secs,
                    duration_secs: position.duration_secs,
                    ended: position.ended,
                    buffering: position.buffering,
                    video_gap: position.video_gap,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn seek(&self, position_ms: u64) -> Result<u64, String> {
        self.session
            .seek(position_ms as f64 / 1000.0)
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
