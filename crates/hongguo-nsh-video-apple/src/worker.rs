use crate::{
    AudioOutput, ImportedFrame, OutputError,
    controller::{Layers, SeekCommand, Shared, Viewport},
    render::Renderer,
};
use hongguo_enhancement::FrameBudget;
use hongguo_nsh_video_api::{
    Enhancement, EnhancementPhase, ErrorCode, Seek, Source, VideoEvent, VideoProps,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::CATransaction;
use std::{
    sync::{Arc, atomic::Ordering, mpsc::Receiver},
    time::{Duration, Instant},
};

type Sink = Arc<dyn Fn(VideoEvent) + Send + Sync>;
const FRAME_TICK: Duration = Duration::from_millis(16);
mod direct;

struct Worker {
    shared: Arc<Shared>,
    layers: Layers,
    emit: Sink,
    props: VideoProps,
    viewport: Viewport,
    revision: u64,
    source: Option<Source>,
    video: Option<AudioOutput>,
    renderer: Option<Renderer>,
    frame: Option<ImportedFrame>,
    pending_seek: Option<(Seek, Instant)>,
    last_metadata: Option<(Option<u64>, u32, u32)>,
    buffered: bool,
    video_gap: bool,
    progress_at: Instant,
    dirty: bool,
    gpu_rebuilds: u8,
    budget: FrameBudget,
    phase: Option<(EnhancementPhase, Enhancement, Option<ErrorCode>, String)>,
    direct: Option<direct::Direct>,
    #[cfg(feature = "test-support")]
    presentation_context: Option<crate::diagnostics::PresentationContext>,
}

pub(crate) fn run(shared: Arc<Shared>, layers: Layers, receiver: Receiver<()>, emit: Sink) {
    layers.set_visible(false);
    let mut worker = Worker {
        shared,
        layers,
        emit,
        props: VideoProps::default(),
        viewport: Viewport::default(),
        revision: 0,
        source: None,
        video: None,
        renderer: None,
        frame: None,
        pending_seek: None,
        last_metadata: None,
        buffered: false,
        video_gap: false,
        progress_at: Instant::now(),
        dirty: false,
        gpu_rebuilds: 0,
        budget: FrameBudget::default(),
        phase: None,
        direct: None,
        #[cfg(feature = "test-support")]
        presentation_context: None,
    };
    while !worker.shared.stop.load(Ordering::Acquire) {
        objc2::rc::autoreleasepool(|_| worker.step());
        if worker.needs_tick() {
            let interval = if worker.shared.display_clock_active.load(Ordering::Acquire) {
                Duration::from_millis(u64::from(worker.props.progress_interval_ms))
            } else {
                FRAME_TICK
            };
            let _ = receiver.recv_timeout(interval);
        } else if receiver.recv().is_err() {
            break;
        }
    }
    worker.layers.set_visible(false);
    worker.video.take();
    worker
        .shared
        .control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
}

impl Worker {
    fn needs_tick(&self) -> bool {
        let retiring = self.renderer.as_ref().is_some_and(Renderer::gpu_pending);
        if !self.props.active {
            return retiring;
        }
        let Some(video) = &self.video else {
            return false;
        };
        if !video.control().frames.is_control_owner().unwrap_or(false) {
            return retiring;
        }
        self.dirty
            || self.pending_seek.is_some()
            || (self.frame.is_none() && !self.video_gap)
            || self
                .renderer
                .as_ref()
                .is_some_and(|renderer| !renderer.idle())
            || self
                .direct
                .as_ref()
                .is_some_and(|direct| direct.frames.holding().unwrap_or(false))
            || (!self.props.paused && !video.ended())
    }
    fn event(&self, event: VideoEvent) {
        if !self.shared.stop.load(Ordering::Acquire) {
            (self.emit)(event);
        }
    }

    fn source_id(&self) -> String {
        self.source
            .as_ref()
            .map(|source| source.id.clone())
            .unwrap_or_default()
    }

    fn status(&mut self, phase: EnhancementPhase, error: Option<&OutputError>) {
        let detail = error
            .map(|error| error.detail().to_owned())
            .unwrap_or_default();
        let settings = self.props.enhancement;
        let code = error.map(OutputError::code);
        if self.phase.as_ref() == Some(&(phase, settings, code, detail.clone())) {
            return;
        }
        self.phase = Some((phase, settings, code, detail.clone()));
        self.event(VideoEvent::EnhancementStatus {
            source_id: self.source_id(),
            settings,
            phase,
            code,
            detail,
        });
    }

    fn fallback(&mut self, error: OutputError) {
        self.direct_failure(error);
    }

    fn step(&mut self) {
        let snapshot = {
            let mut mailbox = self
                .shared
                .mailbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                mailbox.props.clone(),
                mailbox.viewport,
                mailbox.revision,
                mailbox.seek.take(),
            )
        };
        if self.revision != snapshot.2 {
            self.apply(snapshot.0, snapshot.1, snapshot.2);
        }
        if let Some(request) = snapshot.3 {
            self.seek(request);
        }
        if !self.props.active
            || self
                .video
                .as_ref()
                .is_none_or(|video| !video.control().frames.is_control_owner().unwrap_or(false))
        {
            // Old consumers cannot publish/control the replacement session,
            // but their submitted work still needs polling for lease release.
            if let Some(renderer) = &mut self.renderer {
                let _ = renderer.poll();
            }
            return;
        }
        self.confirm_direct_seek();
        self.poll_render();
        self.read_video();
        if self.video_gap {
            return;
        }
        self.prepare_direct();
        if self.dirty && self.frame.is_some() {
            self.draw();
        }
    }

    fn apply(&mut self, props: VideoProps, viewport: Viewport, revision: u64) {
        let changed_source = self.source != props.source || (self.video.is_none() && props.active);
        let changed_active = self.props.active != props.active;
        let changed_mode = self.props.enhancement.anime4k_on != props.enhancement.anime4k_on
            || self.props.enhancement.sharpen_on != props.enhancement.sharpen_on;
        if changed_mode {
            self.budget = FrameBudget::default();
        }
        self.props = props;
        self.revision = revision;
        self.dirty = true;
        if self.viewport != viewport {
            self.viewport = viewport;
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let frame = CGRect::new(
                CGPoint::ZERO,
                CGSize::new(viewport.width_points, viewport.height_points),
            );
            self.layers.metal.setFrame(frame);
            self.layers.metal.setContentsScale(viewport.scale_factor);
            CATransaction::commit();
            // WGPU can leave an implicit outer transaction on this thread without a Cocoa run loop.
            CATransaction::flush();
        }
        if changed_source {
            self.shared.preferred_frame_rate.store(0, Ordering::Release);
            self.layers.set_visible(false);
            self.video.take();
            self.renderer.take();
            self.frame.take();
            self.pending_seek.take();
            self.direct.take();
            self.source = self.props.source.clone();
            self.last_metadata = None;
            self.video_gap = false;
            self.phase = None;
            self.gpu_rebuilds = 0;
            self.budget = FrameBudget::default();
            if let Some(source) = &self.source
                && self.props.active
            {
                self.event(VideoEvent::LoadStart {
                    source_id: source.id.clone(),
                });
                match AudioOutput::open_audio(source.frame_source.as_deref().unwrap_or_default()) {
                    Ok(video) => {
                        if source.frame_source.is_some() {
                            match direct::Direct::open(video.control().frames.clone()) {
                                Ok(direct) => self.direct = Some(direct),
                                Err(error) => {
                                    self.event(VideoEvent::Error {
                                        source_id: source.id.clone(),
                                        code: ErrorCode::DecoderFailed,
                                        detail: error,
                                    });
                                    return;
                                }
                            }
                            video.hold(true);
                        } else {
                            self.event(VideoEvent::Error {
                                source_id: source.id.clone(),
                                code: ErrorCode::InvalidProps,
                                detail: "missing builtin video frame source".into(),
                            });
                            return;
                        }
                        let control = video.control();
                        let mut target = self
                            .shared
                            .control
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let current = self
                            .shared
                            .mailbox
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .props
                            .clone();
                        if current.source == self.source
                            && !self.shared.stop.load(Ordering::Acquire)
                        {
                            control.apply(&current);
                        } else {
                            control.pause();
                        }
                        *target = Some((source.id.clone(), control));
                        drop(target);
                        if let Some(direct) = &self.direct {
                            match direct.frames.configure_decoder_if_owner(Some(
                                crate::native::AppleDevice::lease(),
                            )) {
                                Ok(true) => {}
                                Ok(false) => return,
                                Err(error) => {
                                    self.event(VideoEvent::Error {
                                        source_id: source.id.clone(),
                                        code: ErrorCode::DecoderFailed,
                                        detail: error,
                                    });
                                    return;
                                }
                            }
                        }
                        self.video = Some(video);
                    }
                    Err(error) => self.event(VideoEvent::Error {
                        source_id: source.id.clone(),
                        code: error.code(),
                        detail: error.detail().to_owned(),
                    }),
                }
            } else {
                self.shared
                    .control
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
            }
        }
        if changed_mode || changed_active || !self.props.active {
            self.layers.set_visible(false);
        }
        if !self.props.active {
            self.renderer.take();
            self.frame.take();
            self.status(EnhancementPhase::Idle, None);
        } else if changed_mode || changed_active {
            self.status(EnhancementPhase::Loading, None);
        }
    }

    fn seek(&mut self, command: SeekCommand) {
        let request = command.request;
        if self
            .source
            .as_ref()
            .is_none_or(|source| source.id != request.source_id)
        {
            return;
        }
        self.layers.set_visible(false);
        self.frame.take();
        if self.direct.is_none() {
            self.event(VideoEvent::Error {
                source_id: request.source_id,
                code: ErrorCode::SeekFailed,
                detail: "native playback session is unavailable".into(),
            });
            return;
        }
        if let Some(direct) = &mut self.direct {
            match direct.seek(request.position_ms, command.generation) {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    self.direct_failure(OutputError::new(ErrorCode::SeekFailed, error));
                    return;
                }
            }
            if let Some(video) = &self.video {
                video.hold(true);
            }
        }
        self.pending_seek = Some((request, Instant::now()));
    }

    fn poll_render(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let completed = renderer.poll();
        #[cfg(feature = "test-support")]
        if let Ok(Some(completed)) = &completed {
            self.observe_presentation(completed);
        }
        match completed {
            Ok(Some(completed)) if completed.epoch == self.revision => {
                if !completed.displayed {
                    if let Some(direct) = &mut self.direct {
                        direct.submitted = None;
                    }
                    self.dirty = true;
                    return;
                }
                self.present_direct();
                if self.video.is_none() {
                    return;
                }
                if (self.props.enhancement.anime4k_on || self.props.enhancement.sharpen_on)
                    && self.budget.observe(completed.elapsed)
                {
                    self.fallback(OutputError::new(
                        ErrorCode::EnhancementSlow,
                        "GPU enhancement exceeded consecutive frame budget",
                    ));
                    return;
                }
                self.status(
                    if self.props.enhancement.anime4k_on || self.props.enhancement.sharpen_on {
                        EnhancementPhase::Ready
                    } else {
                        EnhancementPhase::Idle
                    },
                    None,
                );
            }
            Ok(_) => {}
            Err(error) => self.fallback(error),
        }
    }

    fn read_video(&mut self) {
        let Some(video) = &self.video else {
            return;
        };
        let source_id = self.source_id();
        let gap = video.video_gap();
        if gap {
            self.layers.set_visible(false);
            self.frame.take();
            self.dirty = false;
            if let Some(direct) = &mut self.direct {
                direct.blank();
            }
            if video.hold(false) {
                self.shared.reconcile_playback(&source_id);
            }
        }
        self.video_gap = gap;
        let duration_ms = video.duration_ms();
        if self.last_metadata.is_none()
            && let Ok(Some(info)) = video.control().frames.info()
        {
            self.last_metadata = Some((duration_ms, info.width, info.height));
            self.event(VideoEvent::Load {
                source_id: source_id.clone(),
                duration_ms,
                width: info.width,
                height: info.height,
            });
        }
        if let Some((previous, width, height)) = self.last_metadata
            && previous != duration_ms
        {
            self.last_metadata = Some((duration_ms, width, height));
            self.event(VideoEvent::Load {
                source_id: source_id.clone(),
                duration_ms,
                width,
                height,
            });
        }
        if video.take_ended() {
            self.event(VideoEvent::End {
                source_id: source_id.clone(),
            });
        }
        let buffering = video.buffering()
            || self
                .direct
                .as_ref()
                .is_some_and(|direct| direct.frames.holding().unwrap_or(true));
        if buffering != self.buffered {
            self.buffered = buffering;
            self.event(VideoEvent::Buffer {
                source_id: source_id.clone(),
                is_buffering: buffering,
            });
        }
        if self.progress_at.elapsed()
            >= Duration::from_millis(u64::from(self.props.progress_interval_ms))
        {
            self.progress_at = Instant::now();
            if let Some(position_ms) = video.position_ms() {
                self.event(VideoEvent::Progress {
                    source_id: source_id.clone(),
                    position_ms,
                    duration_ms,
                });
            }
        }
        if !gap {
            self.read_direct();
        }
    }

    #[cfg(feature = "test-support")]
    fn observe_presentation(&mut self, completed: &crate::render::Completed) {
        let Some(timing) = completed.presentation_timing else {
            return;
        };
        let Some(context) = self.presentation_context.take() else {
            return;
        };
        let observer = self
            .shared
            .presentation_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(observer) = observer {
            observer(context.observe(timing, completed.elapsed));
        }
    }

    fn draw(&mut self) {
        // Hidden Metal layers can finish GPU work without displaying the drawable.
        // Reveal before acquisition; the audio gate waits for native presentation.
        self.layers.set_visible(self.props.active);
        let frame = self.frame.as_ref().expect("caller checked current frame");
        let shared = self.shared.clone();
        let revision = self.revision;
        let valid = move || {
            !shared.stop.load(Ordering::Acquire)
                && shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .revision
                    == revision
        };
        let result = pollster::block_on(
            self.renderer
                .as_mut()
                .expect("current frame has a renderer")
                .draw(
                    frame,
                    self.viewport,
                    self.props.enhancement,
                    self.props.resize_mode,
                    revision,
                    valid,
                ),
        );
        match result {
            Ok(true) => {
                #[cfg(feature = "test-support")]
                {
                    let metadata = frame.metadata();
                    let exact = self
                        .direct
                        .as_ref()
                        .and_then(|direct| direct.native.as_ref())
                        .filter(|native| {
                            native.generation == metadata.generation
                                && native.serial == metadata.serial
                        });
                    self.presentation_context = Some(crate::diagnostics::PresentationContext {
                        source_id: self.source_id(),
                        revision,
                        generation: Some(metadata.generation),
                        serial: Some(metadata.serial),
                        pts_secs: exact.map(|native| native.pts_secs),
                    });
                }
                if let Some(direct) = &mut self.direct {
                    direct.submitted = direct
                        .native
                        .as_ref()
                        .map(|frame| (frame.generation, frame.serial))
                        .or_else(|| {
                            direct
                                .pixels
                                .as_ref()
                                .map(|frame| (frame.generation, frame.serial))
                        });
                }
                self.dirty = false;
            }
            Ok(false) => {}
            Err(error) => self.fallback(error),
        }
    }
}
