use hongguo_nsh_video_api::{
    ErrorCode,
    frames::{PlaybackPosition, open_frames},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug)]
pub struct OutputError {
    code: ErrorCode,
    detail: String,
    decoder_surface: bool,
}

impl OutputError {
    pub fn code(&self) -> ErrorCode {
        self.code
    }
    pub fn detail(&self) -> &str {
        &self.detail
    }
    pub(crate) fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            decoder_surface: false,
        }
    }
    pub(crate) fn surface(detail: String) -> Self {
        Self {
            code: ErrorCode::FrameFormatUnsupported,
            detail,
            decoder_surface: true,
        }
    }
    pub(crate) fn is_decoder_surface(&self) -> bool {
        self.decoder_surface
    }
}
impl std::fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}
impl std::error::Error for OutputError {}

pub(crate) struct AudioOutput {
    control: crate::control::PlaybackControl,
    ended: AtomicBool,
}
impl AudioOutput {
    pub fn control(&self) -> crate::control::PlaybackControl {
        self.control.clone()
    }
    pub fn hold(&self, held: bool) -> bool {
        self.control.held.swap(held, Ordering::AcqRel) != held
    }
    pub fn ready(&self) -> Result<bool, OutputError> {
        if let Some(error) = self
            .control
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        self.control
            .frames
            .info()
            .map(|info| info.is_some())
            .map_err(decoder_error)
    }
    pub fn open_audio(frame_url: &str) -> Result<Self, OutputError> {
        let frames = Arc::new(open_frames(frame_url).map_err(decoder_error)?);
        Ok(Self {
            control: crate::control::PlaybackControl {
                frames,
                held: Arc::new(AtomicBool::new(true)),
                error: Arc::new(Mutex::new(None)),
                intent: Arc::new(Mutex::new(hongguo_nsh_video_api::frames::PlaybackControl {
                    playing: false,
                    rate: 1.0,
                    volume: 1.0,
                    muted: false,
                })),
            },
            ended: AtomicBool::new(false),
        })
    }
    pub fn position_ms(&self) -> Option<u64> {
        self.position()
            .map(|position| (position.position_secs.max(0.0) * 1000.0).round() as u64)
    }
    pub fn duration_ms(&self) -> Option<u64> {
        self.position()
            .map(|position| (position.duration_secs.max(0.0) * 1000.0).round() as u64)
    }
    pub fn take_ended(&self) -> bool {
        let ended = self.position().is_some_and(|position| position.ended);
        let previous = self.ended.swap(ended, Ordering::AcqRel);
        ended && !previous
    }
    pub fn buffering(&self) -> bool {
        self.position().is_some_and(|position| position.buffering)
            || self.control.frames.holding().unwrap_or(true)
    }
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
    pub fn video_gap(&self) -> bool {
        self.position().is_some_and(|position| position.video_gap)
    }
    fn position(&self) -> Option<PlaybackPosition> {
        match self.control.frames.position() {
            Ok(position) => position,
            Err(error) => {
                self.control.publish_error(decoder_error(error));
                None
            }
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.control.pause();
    }
}
fn decoder_error(error: String) -> OutputError {
    OutputError::new(ErrorCode::DecoderFailed, error)
}
