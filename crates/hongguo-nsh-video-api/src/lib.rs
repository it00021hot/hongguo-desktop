use serde::{Deserialize, Serialize};

pub mod frames;

pub const COMPONENT_NAME: &str = "hongguo.pixel-video";
pub const EVENT_NAME: &str = "video";
pub const STATE_PROP: &str = "state";
pub const SEEK_COMMAND: &str = "seek";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameMetadata {
    pub generation: u64,
    pub serial: u64,
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
    /// Pixel width / height after applying the display rotation.
    pub sample_aspect_ratio: (u32, u32),
}

impl Default for FrameMetadata {
    fn default() -> Self {
        Self {
            generation: 0,
            serial: 0,
            pts_ms: 0,
            width: 0,
            height: 0,
            sample_aspect_ratio: (1, 1),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Source {
    pub id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_source: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResizeMode {
    #[default]
    Contain,
    Cover,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Enhancement {
    pub anime4k_on: bool,
    pub sharpen_on: bool,
    pub strength: u8,
}

impl Default for Enhancement {
    fn default() -> Self {
        Self {
            anime4k_on: false,
            sharpen_on: false,
            strength: 50,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoProps {
    pub source: Option<Source>,
    pub paused: bool,
    pub muted: bool,
    pub volume: f32,
    pub rate: f64,
    pub resize_mode: ResizeMode,
    pub progress_interval_ms: u32,
    pub enhancement: Enhancement,
    pub active: bool,
}

impl Default for VideoProps {
    fn default() -> Self {
        Self {
            source: None,
            paused: true,
            muted: false,
            volume: 1.0,
            rate: 1.0,
            resize_mode: ResizeMode::Contain,
            progress_interval_ms: 250,
            enhancement: Enhancement::default(),
            active: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Seek {
    pub source_id: String,
    pub request_id: String,
    pub position_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    InvalidProps,
    DecoderUnavailable,
    DecoderFailed,
    FrameUnavailable,
    FrameFormatUnsupported,
    GpuUnavailable,
    GpuFailed,
    EnhancementSlow,
    InputTooLarge,
    SeekFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnhancementPhase {
    Idle,
    Loading,
    Ready,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum VideoEvent {
    LoadStart {
        source_id: String,
    },
    Load {
        source_id: String,
        duration_ms: Option<u64>,
        width: u32,
        height: u32,
    },
    Progress {
        source_id: String,
        position_ms: u64,
        duration_ms: Option<u64>,
    },
    Buffer {
        source_id: String,
        is_buffering: bool,
    },
    End {
        source_id: String,
    },
    Seeked {
        source_id: String,
        request_id: String,
        position_ms: u64,
    },
    Error {
        source_id: String,
        code: ErrorCode,
        detail: String,
    },
    EnhancementStatus {
        source_id: String,
        settings: Enhancement,
        phase: EnhancementPhase,
        code: Option<ErrorCode>,
        detail: String,
    },
}
