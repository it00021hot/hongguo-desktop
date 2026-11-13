use crate::{
    DecodedFrame, ImportedFrame, OutputError, VideoOutput,
    controller::{Layers, Shared, Viewport},
    render::Renderer,
};
use hongguo_enhancement::FrameBudget;
use hongguo_nsh_video_api::{
    Enhancement, EnhancementPhase, ErrorCode, Seek, Source, VideoEvent, VideoProps,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::CATransaction;
use std::{
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

type Sink = Arc<dyn Fn(VideoEvent) + Send + Sync>;
const FRAME_TICK: Duration = Duration::from_millis(16);

struct Worker {
    shared: Arc<Shared>,
    layers: Layers,
    emit: Sink,
    props: VideoProps,
    viewport: Viewport,
    revision: u64,
    source: Option<Source>,
    video: Option<VideoOutput>,
    renderer: Option<Renderer>,
    decoded: Option<DecodedFrame>,
    frame: Option<ImportedFrame>,
    pending_seek: Option<(Seek, Receiver<bool>, Instant)>,
    last_metadata: Option<(Option<u64>, u32, u32)>,
    buffered: bool,
    progress_at: Instant,
    dirty: bool,
    failed: Option<Enhancement>,
    budget: FrameBudget,
    phase: Option<(EnhancementPhase, Enhancement, Option<ErrorCode>, String)>,
}

pub(crate) fn run(shared: Arc<Shared>, layers: Layers, receiver: Receiver<()>, emit: Sink) {
    layers.show_enhanced(false, false);
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
        decoded: None,
        frame: None,
        pending_seek: None,
        last_metadata: None,
        buffered: false,
        progress_at: Instant::now(),
        dirty: false,
        failed: None,
        budget: FrameBudget::default(),
        phase: None,
    };
    while !worker.shared.stop.load(Ordering::Acquire) {
        objc2::rc::autoreleasepool(|_| worker.step());
        let _ = receiver.recv_timeout(FRAME_TICK);
    }
    worker.layers.show_enhanced(false, false);
    worker.video.take();
    worker
        .shared
        .control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
}

impl Worker {
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
        self.layers.show_enhanced(false, self.props.active);
        self.failed = Some(self.props.enhancement);
        self.renderer.take();
        self.frame.take();
        if self.props.enhancement.anime4k_on || self.props.enhancement.sharpen_on {
            self.status(EnhancementPhase::Error, Some(&error));
        } else {
            self.status(EnhancementPhase::Idle, None);
        }
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
        if !self.props.active || self.video.is_none() {
            return;
        }
        self.confirm_seek();
        self.poll_render();
        self.read_video();
        self.prepare_frame();
        if self.dirty && self.failed != Some(self.props.enhancement) && self.frame.is_some() {
            self.draw();
        }
    }

    fn apply(&mut self, props: VideoProps, viewport: Viewport, revision: u64) {
        let changed_source = self.source != props.source;
        let changed_active = self.props.active != props.active;
        let changed_mode = self.props.enhancement.anime4k_on != props.enhancement.anime4k_on
            || self.props.enhancement.sharpen_on != props.enhancement.sharpen_on;
        if changed_mode {
            self.budget = FrameBudget::default();
        }
        if self.props.enhancement != props.enhancement || (changed_active && props.active) {
            self.failed = None;
        }
        self.props = props;
        self.revision = revision;
        self.dirty = true;
        self.layers.original.set_resize_mode(self.props.resize_mode);
        if self.viewport != viewport {
            self.viewport = viewport;
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let frame = CGRect::new(
                CGPoint::ZERO,
                CGSize::new(viewport.width_points, viewport.height_points),
            );
            self.layers.original.layer().setFrame(frame);
            self.layers.metal.setFrame(frame);
            self.layers.metal.setContentsScale(viewport.scale_factor);
            CATransaction::commit();
        }
        if changed_source {
            self.layers.show_enhanced(false, self.props.active);
            self.video.take();
            self.renderer.take();
            self.decoded.take();
            self.frame.take();
            self.pending_seek.take();
            self.source = self.props.source.clone();
            self.last_metadata = None;
            self.phase = None;
            self.failed = None;
            self.budget = FrameBudget::default();
            if let Some(source) = &self.source {
                self.event(VideoEvent::LoadStart {
                    source_id: source.id.clone(),
                });
                match VideoOutput::open(&source.url) {
                    Ok(video) => {
                        video.attach_original(&self.layers.original);
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
                        if current.source == self.source {
                            control.apply(&current);
                        } else {
                            control.pause();
                        }
                        *target = Some((source.id.clone(), control));
                        drop(target);
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
            self.layers.show_enhanced(false, self.props.active);
        }
        if !self.enhanced() || !self.props.active {
            // Original AVPlayerLayer remains independent of GPU availability, even
            // while the window is occluded or a previous device has failed.
            self.renderer.take();
            self.frame.take();
            self.status(EnhancementPhase::Idle, None);
        } else if changed_mode || changed_active {
            self.status(EnhancementPhase::Loading, None);
        }
    }

    fn seek(&mut self, request: Seek) {
        if self
            .source
            .as_ref()
            .is_none_or(|source| source.id != request.source_id)
        {
            return;
        }
        self.layers.show_enhanced(false, self.props.active);
        self.frame.take();
        self.decoded.take();
        if let Some(video) = &self.video {
            match video.seek(request.position_ms) {
                Ok(completion) => self.pending_seek = Some((request, completion, Instant::now())),
                Err(error) => self.event(VideoEvent::Error {
                    source_id: self.source_id(),
                    code: error.code(),
                    detail: error.detail().to_owned(),
                }),
            }
        }
    }

    fn confirm_seek(&mut self) {
        let Some((request, completion, started)) = &self.pending_seek else {
            return;
        };
        match completion.try_recv() {
            Ok(true) => {
                if let Some(position_ms) = self.video.as_ref().and_then(VideoOutput::position_ms) {
                    self.event(VideoEvent::Seeked {
                        source_id: request.source_id.clone(),
                        request_id: request.request_id.clone(),
                        position_ms,
                    });
                    self.shared.reconcile_playback(&request.source_id);
                }
                self.pending_seek = None;
            }
            Ok(false) | Err(TryRecvError::Disconnected) => {
                self.event(VideoEvent::Error {
                    source_id: request.source_id.clone(),
                    code: ErrorCode::SeekFailed,
                    detail: "AVPlayer did not finish the current seek".into(),
                });
                self.pending_seek = None;
            }
            Err(TryRecvError::Empty) if started.elapsed() >= Duration::from_secs(15) => {
                self.event(VideoEvent::Error {
                    source_id: request.source_id.clone(),
                    code: ErrorCode::SeekFailed,
                    detail: "AVPlayer seek completion timed out".into(),
                });
                self.pending_seek = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn poll_render(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match renderer.poll() {
            Ok(Some(completed)) if completed.epoch == self.revision => {
                if (self.props.enhancement.anime4k_on || self.props.enhancement.sharpen_on)
                    && self.budget.observe(completed.elapsed)
                {
                    self.fallback(OutputError::new(
                        ErrorCode::EnhancementSlow,
                        "GPU enhancement exceeded consecutive frame budget",
                    ));
                    return;
                }
                self.layers.show_enhanced(true, self.props.active);
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
        let duration_ms = video.duration_ms();
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
        let buffering = video.buffering();
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
        match video.next_frame() {
            Ok(Some(frame)) => {
                let metadata = frame.metadata();
                let loaded = (duration_ms, metadata.width, metadata.height);
                if self.last_metadata != Some(loaded) {
                    self.last_metadata = Some(loaded);
                    self.event(VideoEvent::Load {
                        source_id,
                        duration_ms,
                        width: metadata.width,
                        height: metadata.height,
                    });
                }
                self.decoded = Some(frame);
                self.frame.take();
                self.dirty = true;
            }
            Ok(None) => {}
            Err(error) => {
                self.event(VideoEvent::Error {
                    source_id,
                    code: error.code(),
                    detail: error.detail().to_owned(),
                });
                self.video.take();
            }
        }
    }

    fn enhanced(&self) -> bool {
        self.props.enhancement.anime4k_on || self.props.enhancement.sharpen_on
    }

    fn prepare_frame(&mut self) {
        if !self.enhanced() || self.failed == Some(self.props.enhancement) || self.frame.is_some() {
            return;
        }
        let Some(decoded) = self.decoded.clone() else {
            return;
        };
        if self.renderer.is_none() {
            self.status(EnhancementPhase::Loading, None);
            match pollster::block_on(Renderer::new(self.layers.metal.clone())) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    self.fallback(error);
                    return;
                }
            }
        }
        match self
            .renderer
            .as_ref()
            .expect("renderer initialized")
            .import(decoded)
        {
            Ok(frame) => {
                self.frame = Some(frame);
                self.dirty = true;
            }
            Err(error) => self.fallback(error),
        }
    }

    fn draw(&mut self) {
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
            Ok(true) => self.dirty = false,
            Ok(false) => {}
            Err(error) => self.fallback(error),
        }
    }
}
