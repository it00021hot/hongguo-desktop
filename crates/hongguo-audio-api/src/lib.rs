use std::{error::Error, fmt};

pub mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

impl PcmFormat {
    pub fn validate(self) -> Result<Self, AudioError> {
        if self.sample_rate == 0 || self.channels == 0 {
            return Err(AudioError::invalid_format(
                "PCM sample rate and channel count must be positive",
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadResult {
    pub media_frames: u32,
    pub epoch: u64,
}

pub trait PcmSource: Send + Sync {
    /// Fills interleaved f32 PCM with silence after `media_frames`; must not block or allocate.
    fn read(&self, output: &mut [f32]) -> ReadResult;

    /// Device-consumed frames, including silence; must not block or allocate.
    fn played(&self, _epoch: u64, _device_frames: u64) {}
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub epoch: u64,
    pub device_frames: u64,
    pub latency_frames: u32,
}

pub trait AudioOutput: Send {
    fn start(&mut self) -> Result<(), AudioError>;
    fn pause(&mut self) -> Result<(), AudioError>;
    fn flush(&mut self, epoch: u64) -> Result<(), AudioError>;
    fn position(&self) -> Result<Position, AudioError>;
    fn format(&self) -> PcmFormat;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioErrorKind {
    Device,
    InvalidFormat,
    Callback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError {
    kind: AudioErrorKind,
    message: String,
}

impl AudioError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: AudioErrorKind::Device,
            message: message.into(),
        }
    }
    pub fn invalid_format(message: impl Into<String>) -> Self {
        Self {
            kind: AudioErrorKind::InvalidFormat,
            message: message.into(),
        }
    }
    pub fn callback(message: impl Into<String>) -> Self {
        Self {
            kind: AudioErrorKind::Callback,
            message: message.into(),
        }
    }
    pub fn kind(&self) -> AudioErrorKind {
        self.kind
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AudioError {}
