use crate::{OriginalLayer, OutputError};
use hongguo_nsh_video_api::{ErrorCode, Seek, VideoEvent, VideoProps};
use objc2::rc::Retained;
use objc2_quartz_core::{CAMetalLayer, CATransaction};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{SyncSender, TrySendError},
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Viewport {
    pub width_px: u32,
    pub height_px: u32,
    pub width_points: f64,
    pub height_points: f64,
    pub scale_factor: f64,
}

pub(crate) struct Mailbox {
    pub props: VideoProps,
    pub viewport: Viewport,
    pub seek: Option<Seek>,
    pub revision: u64,
}

pub(crate) struct Shared {
    pub mailbox: Mutex<Mailbox>,
    pub stop: AtomicBool,
    pub stopped: AtomicBool,
    pub control: Mutex<Option<(String, crate::control::PlaybackControl)>>,
}

impl Shared {
    pub fn reconcile_playback(&self, source_id: &str) {
        // Serialize with UI pause/rate writes and read the latest intent after a
        // seek. AVPlayer itself pauses at EOF without changing our desired props.
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
            control.apply(&props);
        }
    }
}

pub(crate) struct Layers {
    pub original: OriginalLayer,
    pub metal: Retained<CAMetalLayer>,
}

// SAFETY: Only the render worker changes these child layers; AppKit/UIKit owns the separate parent view.
unsafe impl Send for Layers {}

impl Layers {
    pub fn show_enhanced(&self, enhanced: bool, active: bool) {
        // The worker has no Cocoa run loop to commit implicit transactions.
        // Explicit commits make the same-player raw/Metal handoff atomic.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.metal.setHidden(!enhanced || !active);
        self.original.layer().setHidden(enhanced || !active);
        CATransaction::commit();
    }
}

pub struct PlayerController {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}

impl PlayerController {
    pub fn spawn(
        original: OriginalLayer,
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
        });
        let (wake, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = shared.clone();
        let layers = Layers { original, metal };
        std::thread::Builder::new()
            .name("hongguo-pixel-video".into())
            .spawn(move || {
                crate::worker::run(worker.clone(), layers, receiver, Arc::new(emit));
                worker.stopped.store(true, Ordering::Release);
            })
            .map_err(|error| OutputError::new(ErrorCode::DecoderUnavailable, error.to_string()))?;
        Ok(Self { shared, wake })
    }

    pub fn set_props(&self, props: VideoProps) -> Result<(), OutputError> {
        if !props.rate.is_finite()
            || props.rate <= 0.0
            || props.rate > f64::from(f32::MAX)
            || !props.volume.is_finite()
            || !(0.0..=1.0).contains(&props.volume)
            || props.progress_interval_ms == 0
            || props.enhancement.strength > 100
            || props
                .source
                .as_ref()
                .is_some_and(|source| source.id.is_empty() || source.url.is_empty())
        {
            return Err(OutputError::new(
                ErrorCode::InvalidProps,
                "invalid native video state",
            ));
        }
        // Serializes publication with the initial AVPlayer control handoff. No GPU work
        // or asynchronous operation holds this lock; a pause cannot wait for compilation.
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
            let mut mailbox = self.shared.mailbox.lock().map_err(poisoned)?;
            if mailbox
                .props
                .source
                .as_ref()
                .is_none_or(|source| source.id != request.source_id)
            {
                return Ok(());
            }
            mailbox.seek = Some(request);
            mailbox.revision = mailbox.revision.wrapping_add(1);
        }
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
