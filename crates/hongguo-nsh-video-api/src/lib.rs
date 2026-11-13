use serde::{Deserialize, Serialize};

pub mod frames;

pub const COMPONENT_NAME: &str = "hongguo.pixel-video";
pub const EVENT_NAME: &str = "video";
pub const STATE_PROP: &str = "state";
pub const SEEK_COMMAND: &str = "seek";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameMetadata {
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhancement_status_requires_the_producers_exact_settings_snapshot() {
        let settings = Enhancement {
            anime4k_on: true,
            sharpen_on: true,
            strength: 73,
        };
        let event = VideoEvent::EnhancementStatus {
            source_id: "source-2".into(),
            settings,
            phase: EnhancementPhase::Ready,
            code: None,
            detail: String::new(),
        };
        let mut wire = serde_json::to_value(&event).expect("typed enhancement status");
        assert_eq!(
            wire["payload"]["settings"],
            serde_json::json!({"anime4kOn": true, "sharpenOn": true, "strength": 73})
        );
        let decoded: VideoEvent =
            serde_json::from_value(wire.clone()).expect("status snapshot round trip");
        assert!(
            matches!(decoded, VideoEvent::EnhancementStatus { settings: actual, .. } if actual == settings)
        );
        wire["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove("settings");
        assert!(serde_json::from_value::<VideoEvent>(wire).is_err());
    }

    #[test]
    fn an_unknown_native_duration_is_distinct_from_a_finite_zero_duration() {
        let event = |duration_ms| VideoEvent::Progress {
            source_id: "event-hls".into(),
            position_ms: 5_000,
            duration_ms,
        };
        let unknown = serde_json::to_value(event(None)).expect("unknown duration event");
        let finite_zero = serde_json::to_value(event(Some(0))).expect("finite zero duration event");
        assert!(unknown["payload"]["durationMs"].is_null());
        assert_eq!(finite_zero["payload"]["durationMs"], 0);
        assert!(matches!(
            serde_json::from_value::<VideoEvent>(unknown).expect("unknown duration round trip"),
            VideoEvent::Progress {
                position_ms: 5_000,
                duration_ms: None,
                ..
            }
        ));
    }

    #[test]
    fn seek_confirmation_keeps_source_and_request_identity_as_strings() {
        let event = VideoEvent::Seeked {
            source_id: "9223372036854775807".into(),
            request_id: "seek-2".into(),
            position_ms: 1_250,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "seeked");
        assert_eq!(json["payload"]["sourceId"], "9223372036854775807");
        assert_eq!(json["payload"]["requestId"], "seek-2");
        assert_eq!(json["payload"]["positionMs"], 1_250);
        assert_eq!(serde_json::from_value::<VideoEvent>(json).unwrap(), event);
    }

    #[test]
    fn default_props_do_not_start_a_decoder_or_enable_enhancement() {
        let props = VideoProps::default();
        assert!(props.source.is_none());
        assert!(!props.active);
        assert!(props.paused);
        assert!(!props.enhancement.anime4k_on);
        assert!(!props.enhancement.sharpen_on);
    }
}
