use crate::presentation::Presentation;
use crate::{DisplayedFrame, VideoError};
use hongguo_media_types::PresentationFrame;
use hongguo_nsh_video_api::{EVENT_NAME, ErrorCode, FrameMetadata, VideoEvent, VideoProps};
use nsh_platform_windows::ComponentContext;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

struct RetainedFrame {
    query: Presentation,
    _frame: Arc<PresentationFrame>,
}

pub(crate) struct RawSnapshot {
    pub device: ID3D11Device,
    pub texture: ID3D11Texture2D,
    pub metadata: FrameMetadata,
    pub color: Arc<hongguo_video_color::ColorTransform>,
    pub output_mode: nsh_platform_windows::OutputColorMode,
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
    pub awaiting_presentation: AtomicBool,
    pub consumer_active: AtomicBool,
    pub video_gap: AtomicBool,
    pub wake_gpu: Condvar,
    retained: Mutex<Vec<RetainedFrame>>,
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
            awaiting_presentation: AtomicBool::new(true),
            consumer_active: AtomicBool::new(true),
            video_gap: AtomicBool::new(false),
            wake_gpu: Condvar::new(),
            retained: Mutex::new(Vec::new()),
        })
    }

    pub fn emit(&self, event: VideoEvent) {
        if self.stopped.load(Ordering::Acquire) || !self.consumer_active.load(Ordering::Acquire) {
            return;
        }
        match serde_json::to_value(event) {
            Ok(value) => self.context.emit(EVENT_NAME, value),
            Err(error) => log::error!("native video event serialization failed: {error}"),
        }
    }

    pub fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.wake_gpu.notify_one();
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

    pub fn set_video_gap(&self, gap: bool) {
        if self.video_gap.swap(gap, Ordering::AcqRel) != gap {
            self.clear_frames();
        }
    }

    pub fn valid(&self, revision: u64) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self.consumer_active.load(Ordering::Acquire)
            && !self.video_gap.load(Ordering::Acquire)
            && self.revision.load(Ordering::Acquire) == revision
    }

    /// Keep a decoder pool slice leased until its GPU copy/conversion finishes.
    /// This queue outlives a source change and is drained by the frame worker on close.
    pub fn retain(&self, query: Presentation, frame: Arc<PresentationFrame>) {
        query.end();
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RetainedFrame {
                query,
                _frame: frame,
            });
    }

    pub fn reap(&self) -> bool {
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        retained.retain(|pending| match pending.query.completed() {
            Ok(done) => !done,
            Err(error) => {
                log::warn!("native frame GPU retirement failed: {error}");
                false
            }
        });
        !retained.is_empty()
    }
}

pub(crate) fn validate(props: &VideoProps) -> Result<(), VideoError> {
    if !props.rate.is_finite()
        || props.rate <= 0.0
        || !props.volume.is_finite()
        || !(0.0..=1.0).contains(&props.volume)
        || props.progress_interval_ms == 0
        || props.enhancement.strength > 100
        || props.source.as_ref().is_some_and(|source| {
            source.id.is_empty() || source.frame_source.as_deref().is_none_or(str::is_empty)
        })
    {
        return Err(VideoError::new(
            ErrorCode::InvalidProps,
            "invalid native video properties",
        ));
    }
    Ok(())
}
