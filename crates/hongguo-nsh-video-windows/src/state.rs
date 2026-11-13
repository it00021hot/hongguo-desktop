use crate::{DisplayedFrame, VideoError};
use hongguo_nsh_video_api::{EVENT_NAME, ErrorCode, FrameMetadata, VideoEvent, VideoProps};
use nsh_platform_windows::ComponentContext;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

pub(crate) struct RawSnapshot {
    pub device: ID3D11Device,
    pub texture: ID3D11Texture2D,
    pub metadata: FrameMetadata,
}
pub(crate) struct Picture {
    pub revision: u64,
    pub frame: Arc<DisplayedFrame>,
}

pub(crate) struct Shared {
    pub context: ComponentContext,
    pub props: Mutex<VideoProps>,
    pub revision: AtomicU64,
    pub raw: Mutex<Option<RawSnapshot>>,
    pub picture: Mutex<Option<Picture>>,
    pub gpu_error: Mutex<Option<VideoError>>,
    pub startup_error: Mutex<Option<VideoError>>,
    pub stopped: AtomicBool,
}

impl Shared {
    pub fn new(context: ComponentContext) -> Arc<Self> {
        Arc::new(Self {
            context,
            props: Mutex::new(VideoProps::default()),
            revision: AtomicU64::new(0),
            raw: Mutex::new(None),
            picture: Mutex::new(None),
            gpu_error: Mutex::new(None),
            startup_error: Mutex::new(None),
            stopped: AtomicBool::new(false),
        })
    }

    pub fn emit(&self, event: VideoEvent) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        match serde_json::to_value(event) {
            Ok(value) => self.context.emit(EVENT_NAME, value),
            Err(error) => log::error!("native video event serialization failed: {error}"),
        }
    }

    pub fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.picture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    pub fn clear_frames(&self) {
        self.invalidate();
        self.raw
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    pub fn valid(&self, revision: u64) -> bool {
        !self.stopped.load(Ordering::Acquire) && self.revision.load(Ordering::Acquire) == revision
    }
}

pub(crate) fn validate(props: &VideoProps) -> Result<(), VideoError> {
    if !props.rate.is_finite()
        || props.rate <= 0.0
        || !props.volume.is_finite()
        || !(0.0..=1.0).contains(&props.volume)
        || props.progress_interval_ms == 0
        || props.enhancement.strength > 100
        || props
            .source
            .as_ref()
            .is_some_and(|source| source.id.is_empty() || source.url.is_empty())
    {
        return Err(VideoError::new(
            ErrorCode::InvalidProps,
            "invalid native video properties",
        ));
    }
    Ok(())
}
