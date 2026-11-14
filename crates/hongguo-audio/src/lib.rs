pub use hongguo_audio_api::{
    AudioError, AudioErrorKind, AudioOutput, PcmFormat, PcmSource, Position, ReadResult,
};
use std::sync::Arc;

#[cfg(target_os = "android")]
use hongguo_audio_android as platform;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use hongguo_audio_apple as platform;
#[cfg(target_os = "windows")]
use hongguo_audio_windows as platform;

pub struct Output {
    inner: hongguo_audio_api::native::NativeOutput,
}

impl Output {
    pub fn open(format: PcmFormat, source: Arc<dyn PcmSource>) -> Result<Self, AudioError> {
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "android"
        ))]
        return platform::open(format, source).map(|inner| Self { inner });
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "android"
        )))]
        {
            let _ = (format, source);
            Err(AudioError::new("PCM output is unavailable on this target"))
        }
    }
    pub fn start(&mut self) -> Result<(), AudioError> {
        self.inner.start()
    }
    pub fn pause(&mut self) -> Result<(), AudioError> {
        self.inner.pause()
    }
    pub fn flush(&mut self, epoch: u64) -> Result<(), AudioError> {
        self.inner.flush(epoch)
    }
    pub fn position(&self) -> Result<Position, AudioError> {
        self.inner.position()
    }
    pub fn format(&self) -> PcmFormat {
        self.inner.format()
    }
}

impl AudioOutput for Output {
    fn start(&mut self) -> Result<(), AudioError> {
        self.start()
    }
    fn pause(&mut self) -> Result<(), AudioError> {
        self.pause()
    }
    fn flush(&mut self, epoch: u64) -> Result<(), AudioError> {
        self.flush(epoch)
    }
    fn position(&self) -> Result<Position, AudioError> {
        self.position()
    }
    fn format(&self) -> PcmFormat {
        self.format()
    }
}
