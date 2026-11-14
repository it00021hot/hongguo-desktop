use crate::OutputError;
use hongguo_nsh_video_api::{ErrorCode, Seek, VideoEvent, VideoProps};
use objc2::rc::Retained;
use objc2_quartz_core::{CAMetalLayer, CATransaction};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    mpsc::{SyncSender, TrySendError},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewport {
    pub width_px: u32,
    pub height_px: u32,
    pub width_points: f64,
    pub height_points: f64,
    pub scale_factor: f64,
    pub edr_headroom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width_px: 0,
            height_px: 0,
            width_points: 0.0,
            height_points: 0.0,
            scale_factor: 1.0,
            edr_headroom: 1.0,
        }
    }
}

impl Viewport {
    pub fn color_target(self) -> hongguo_video_color::DisplayTarget {
        hongguo_video_color::DisplayTarget {
            primaries: hongguo_video_color::Primaries::BT709,
            sdr_white_nits: hongguo_video_color::SCRGB_WHITE_NITS,
            peak_nits: hongguo_video_color::SCRGB_WHITE_NITS * self.edr_headroom,
            encoding: hongguo_video_color::OutputEncoding::LinearScRgb,
            gamut_mapping: false,
        }
    }
}

pub(crate) struct Mailbox {
    pub props: VideoProps,
    pub viewport: Viewport,
    pub seek: Option<SeekCommand>,
    pub revision: u64,
}

pub(crate) struct SeekCommand {
    pub request: Seek,
    pub generation: Option<u64>,
}

pub(crate) struct Shared {
    pub mailbox: Mutex<Mailbox>,
    pub stop: AtomicBool,
    pub stopped: AtomicBool,
    pub control: Mutex<Option<(String, crate::control::PlaybackControl)>>,
    pub display_clock_active: AtomicBool,
    pub display_deadline: AtomicU64,
    pub preferred_frame_rate: AtomicU32,
    #[cfg(feature = "test-support")]
    pub presentation_observer: Mutex<Option<crate::diagnostics::PresentationObserver>>,
}

impl Shared {
    pub fn reconcile_playback(&self, source_id: &str) {
        // Reconcile the current UI intent after the first frame has reached the GPU.
        let held = self
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((id, control)) = held.as_ref().filter(|(id, _)| id == source_id) else {
            return;
        };
        let props = self
            .mailbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .props
            .clone();
        if self.stop.load(Ordering::Acquire)
            || props.source.as_ref().is_none_or(|source| &source.id != id)
        {
            control.pause();
        } else {
            control.reconcile(&props);
        }
    }
}

pub(crate) struct Layers {
    pub metal: Retained<CAMetalLayer>,
}

// SAFETY: Only the render worker changes these child layers; AppKit/UIKit owns the separate parent view.
unsafe impl Send for Layers {}

impl Layers {
    pub fn set_visible(&self, visible: bool) {
        // The worker has no Cocoa run loop to commit implicit transactions.
        // Explicit commits publish visibility without a Cocoa run loop.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.metal.setHidden(!visible);
        CATransaction::commit();
    }
}

pub struct PlayerController {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}

impl PlayerController {
    pub fn set_display_clock_active(&self, active: bool) {
        self.shared
            .display_clock_active
            .store(active, Ordering::Release);
    }
    pub fn preferred_frame_rate(&self) -> u32 {
        self.shared.preferred_frame_rate.load(Ordering::Acquire)
    }
    pub fn display_tick(&self, deadline: f64) -> Result<(), OutputError> {
        self.shared
            .display_deadline
            .store(deadline.to_bits(), Ordering::Release);
        self.wake()
    }

    pub fn spawn(
        metal: Retained<CAMetalLayer>,
        emit: impl Fn(VideoEvent) + Send + Sync + 'static,
    ) -> Result<Self, OutputError> {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                props: VideoProps::default(),
                viewport: Viewport::default(),
                seek: None,
                revision: 0,
            }),
            stop: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            control: Mutex::new(None),
            display_clock_active: AtomicBool::new(false),
            display_deadline: AtomicU64::new(0),
            preferred_frame_rate: AtomicU32::new(0),
            #[cfg(feature = "test-support")]
            presentation_observer: Mutex::new(None),
        });
        let (wake, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = shared.clone();
        let layers = Layers { metal };
        std::thread::Builder::new()
            .name("hongguo-pixel-video".into())
            .spawn(move || {
                crate::worker::run(worker.clone(), layers, receiver, Arc::new(emit));
                worker.stopped.store(true, Ordering::Release);
            })
            .map_err(|error| OutputError::new(ErrorCode::DecoderUnavailable, error.to_string()))?;
        Ok(Self { shared, wake })
    }

    #[cfg(feature = "test-support")]
    /// The observer runs on the video worker and must not block or control this player.
    pub fn set_presentation_observer(
        &self,
        observer: impl Fn(crate::PresentationObservation) + Send + Sync + 'static,
    ) {
        *self
            .shared
            .presentation_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(observer));
    }

    pub fn set_props(&self, props: VideoProps) -> Result<(), OutputError> {
        if !props.rate.is_finite()
            || props.rate <= 0.0
            || props.rate > f64::from(f32::MAX)
            || !props.volume.is_finite()
            || !(0.0..=1.0).contains(&props.volume)
            || props.progress_interval_ms == 0
            || props.enhancement.strength > 100
            || props.source.as_ref().is_some_and(|source| {
                source.id.is_empty()
                    || source.url.is_empty()
                    || source.frame_source.as_ref().is_none_or(String::is_empty)
            })
        {
            return Err(OutputError::new(
                ErrorCode::InvalidProps,
                "invalid native video state",
            ));
        }
        // Control publication never waits for GPU compilation or device I/O.
        let control = self.shared.control.lock().map_err(poisoned)?;
        let old = {
            let mut mailbox = self.shared.mailbox.lock().map_err(poisoned)?;
            if mailbox.props == props {
                return Ok(());
            }
            let old = std::mem::replace(&mut mailbox.props, props.clone());
            mailbox.revision = mailbox.revision.wrapping_add(1);
            old
        };
        if let Some((id, control)) = control.as_ref() {
            if props.source.as_ref().is_none_or(|source| &source.id != id) {
                control.pause();
            } else if old.paused != props.paused
                || old.active != props.active
                || old.rate != props.rate
                || old.volume != props.volume
                || old.muted != props.muted
            {
                control.apply(&props);
            }
        }
        drop(control);
        self.wake()
    }

    pub fn resize(
        &self,
        width_px: u32,
        height_px: u32,
        width_points: f64,
        height_points: f64,
        scale_factor: f64,
    ) -> Result<(), OutputError> {
        let viewport = Viewport {
            width_px,
            height_px,
            width_points,
            height_points,
            scale_factor,
            edr_headroom: self
                .shared
                .mailbox
                .lock()
                .map_err(poisoned)?
                .viewport
                .edr_headroom,
        };
        if !width_points.is_finite()
            || !height_points.is_finite()
            || !scale_factor.is_finite()
            || width_points < 0.0
            || height_points < 0.0
            || scale_factor <= 0.0
        {
            return Err(OutputError::new(
                ErrorCode::InvalidProps,
                "invalid native video viewport",
            ));
        }
        {
            let mut mailbox = self.shared.mailbox.lock().map_err(poisoned)?;
            if mailbox.viewport == viewport {
                return Ok(());
            }
            mailbox.viewport = viewport;
            mailbox.revision = mailbox.revision.wrapping_add(1);
        }
        self.wake()
    }

    pub fn seek(&self, request: Seek) -> Result<(), OutputError> {
        if request.source_id.is_empty() || request.request_id.is_empty() {
            return Err(OutputError::new(
                ErrorCode::InvalidProps,
                "seek requires source and request identities",
            ));
        }
        {
            let control = self.shared.control.lock().map_err(poisoned)?;
            let mut mailbox = self.shared.mailbox.lock().map_err(poisoned)?;
            if mailbox
                .props
                .source
                .as_ref()
                .is_none_or(|source| source.id != request.source_id)
            {
                return Ok(());
            }
            let generation = if let Some((_, control)) =
                control.as_ref().filter(|(id, _)| *id == request.source_id)
            {
                control.apply(&mailbox.props);
                let Some(generation) = control
                    .frames
                    .seek_if_owner(request.position_ms)
                    .map_err(|error| OutputError::new(ErrorCode::SeekFailed, error))?
                else {
                    return Ok(());
                };
                Some(generation)
            } else {
                None
            };
            mailbox.seek = Some(SeekCommand {
                request,
                generation,
            });
            mailbox.revision = mailbox.revision.wrapping_add(1);
        }
        self.wake()
    }

    pub fn set_display_headroom(&self, headroom: f32) -> Result<(), OutputError> {
        if !headroom.is_finite() || headroom < 1.0 {
            return Err(OutputError::new(
                ErrorCode::InvalidProps,
                "invalid EDR display headroom",
            ));
        }
        let mut mailbox = self.shared.mailbox.lock().map_err(poisoned)?;
        if mailbox.viewport.edr_headroom == headroom {
            return Ok(());
        }
        mailbox.viewport.edr_headroom = headroom;
        mailbox.revision = mailbox.revision.wrapping_add(1);
        drop(mailbox);
        self.wake()
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
        let control = self
            .shared
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|(_, player)| player.clone());
        if let Some(player) = control {
            player.pause();
        }
        let _ = self.wake.try_send(());
    }

    pub fn is_stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
    }

    pub fn source_is_playing(&self, source_id: &str) -> bool {
        if self.shared.stop.load(Ordering::Acquire) {
            return false;
        }
        self.shared
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(id, control)| id == source_id && control.is_playing())
    }

    fn wake(&self) -> Result<(), OutputError> {
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err(OutputError::new(
                ErrorCode::DecoderFailed,
                "native video worker stopped",
            )),
        }
    }
}

impl Drop for PlayerController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn poisoned(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::DecoderFailed, error.to_string())
}
