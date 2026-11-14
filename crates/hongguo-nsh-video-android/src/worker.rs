use crate::{
    bridge::FRAME_TICK,
    controller::{Shared, Viewport},
    render::Renderer,
};
use hongguo_enhancement::FrameBudget;
use hongguo_nsh_video_api::{Enhancement, EnhancementPhase, ErrorCode, VideoEvent, VideoProps};
use std::sync::{Arc, atomic::Ordering, mpsc::Receiver};

#[path = "worker_render.rs"]
mod rendering;

struct Worker {
    shared: Arc<Shared>,
    props: VideoProps,
    generation: u64,
    revision: u64,
    viewport: Viewport,
    renderer: Option<Renderer>,
    failed: Option<(Enhancement, ErrorCode, String)>,
    requested_output: bool,
    video_size: Option<(u32, u32)>,
    dirty: bool,
    status: Option<(Enhancement, EnhancementPhase, Option<ErrorCode>, String)>,
    budget: FrameBudget,
    holding: Option<bool>,
    terminal: bool,
    gpu_recovery: Option<(bool, String)>,
    video_gap: bool,
}
pub(crate) fn run(shared: Arc<Shared>, receiver: Receiver<()>) {
    let vm = shared.bridge.vm.clone();
    let _attachment = match vm.attach_current_thread() {
        Ok(attachment) => attachment,
        Err(error) => {
            log::error!("[Android video] cannot attach render worker: {error}");
            return;
        }
    };
    let mut worker = Worker {
        shared,
        props: VideoProps::default(),
        generation: 0,
        revision: u64::MAX,
        viewport: Viewport::default(),
        renderer: None,
        failed: None,
        requested_output: false,
        video_size: None,
        dirty: true,
        status: None,
        budget: FrameBudget::default(),
        holding: None,
        terminal: false,
        gpu_recovery: None,
        video_gap: false,
    };
    while !worker.shared.stop.load(Ordering::Acquire) {
        worker.step();
        if worker
            .renderer
            .as_ref()
            .is_some_and(|renderer| !renderer.ready())
        {
            let _ = receiver.recv_timeout(FRAME_TICK);
        } else if receiver.recv().is_err() {
            break;
        }
    }
    // Java retires playback controls before destroying this native renderer.
    worker.renderer.take();
}
impl Worker {
    fn direct_lease(&self) -> Option<Arc<hongguo_nsh_video_api::frames::FrameLease>> {
        self.shared
            .direct
            .lock()
            .expect("Android direct source")
            .as_ref()
            .filter(|slot| slot.epoch == self.generation)
            .map(|slot| slot.lease.clone())
    }
    fn options(&self) -> Enhancement {
        if self
            .failed
            .as_ref()
            .is_some_and(|(settings, _, _)| *settings == self.props.enhancement)
        {
            Enhancement::default()
        } else {
            self.props.enhancement
        }
    }
    fn enabled(&self) -> bool {
        self.props.active && !self.terminal && self.props.source.is_some()
    }
    fn current(&self) -> bool {
        !self.shared.stop.load(Ordering::Acquire)
            && self.generation == self.shared.generation.load(Ordering::Acquire)
    }
    fn step(&mut self) {
        let (props, viewport, mut size, seek, revision, generation) = {
            let mut mailbox = self.shared.mailbox.lock().expect("Android video mailbox");
            (
                mailbox.props.clone(),
                mailbox.viewport.clone(),
                mailbox.size,
                mailbox.seek.take(),
                self.shared.revision.load(Ordering::Acquire),
                self.shared.generation.load(Ordering::Acquire),
            )
        };
        if revision != self.revision {
            self.video_gap = false;
            let source_changed = props.source != self.props.source;
            let changed_mode = props.enhancement != self.props.enhancement;
            let props_changed = props != self.props;
            let window_changed = viewport.window.as_ref().map(|window| window.ptr())
                != self.viewport.window.as_ref().map(|window| window.ptr());
            if source_changed {
                self.detach();
            }
            if window_changed && let Some(renderer) = &mut self.renderer {
                renderer.output_changed(viewport.version);
            }
            if window_changed
                && let (Some(renderer), Some(window)) =
                    (&mut self.renderer, viewport.window.clone())
                && let Err(detail) = renderer.surface.rebind(
                    window,
                    viewport.width,
                    viewport.height,
                    viewport.hdr_peak,
                )
            {
                let had_surface = renderer.has_decoder_surface();
                if let Err(error) = self.shared.bridge.direct_holding(self.generation, true) {
                    self.error(ErrorCode::GpuFailed, error);
                    return;
                }
                self.renderer.take();
                self.gpu_recovery = Some((had_surface, detail));
            }
            if source_changed || changed_mode {
                self.failed = None;
                self.status = None;
            }
            if source_changed {
                self.gpu_recovery = None;
                self.terminal = false;
                self.budget = FrameBudget::default();
                self.requested_output = false;
                self.holding = None;
            }
            self.props = props;
            self.generation = generation;
            self.revision = revision;
            self.viewport = viewport;
            self.dirty = true;
            if changed_mode && self.enabled() {
                self.phase(EnhancementPhase::Loading, None, String::new());
            }
            if props_changed {
                match serde_json::to_string(&self.props)
                    .map_err(|error| error.to_string())
                    .and_then(|json| self.shared.bridge.state(&json, self.generation))
                {
                    Ok(()) => {}
                    Err(error) => {
                        self.shared.bridge.clear_exception();
                        self.error(ErrorCode::DecoderFailed, error);
                        return;
                    }
                }
            }
        }
        if self.terminal {
            return;
        }
        if let Some(lease) = self.direct_lease() {
            match lease.is_control_owner() {
                Ok(true) => {}
                Ok(false) => {
                    if let Some(renderer) = &mut self.renderer {
                        let _ = renderer.poll();
                        renderer.binding_lost();
                    }
                    return;
                }
                Err(error) => {
                    self.error(ErrorCode::DecoderFailed, error);
                    return;
                }
            }
            match lease.info() {
                Ok(Some(info)) => size = Some((info.width, info.height)),
                Ok(None) => {}
                Err(error) => {
                    self.error(ErrorCode::DecoderFailed, error);
                    return;
                }
            }
        }
        if !self.enabled() {
            if let Some(renderer) = &mut self.renderer {
                let _ = renderer.poll();
            }
            return;
        }
        if size != self.video_size {
            self.detach();
            self.video_size = size;
        }
        if let Some(seek) = seek
            && let Err(error) = serde_json::to_string(&seek)
                .map_err(|error| error.to_string())
                .and_then(|json| self.shared.bridge.seek(&json, self.generation))
        {
            self.shared.bridge.clear_exception();
            self.error(ErrorCode::SeekFailed, error);
        }
        if self.prepare_renderer(size) {
            if self.viewport.window.is_none() {
                return;
            }
            self.draw();
        }
    }
    fn detach(&mut self) {
        let _ = self.shared.bridge.show(self.generation, false);
        self.renderer.take();
    }
    fn fallback(&mut self, code: ErrorCode, detail: String) {
        self.fallback_for(self.options(), code, detail);
    }
    fn fallback_for(&mut self, settings: Enhancement, code: ErrorCode, detail: String) {
        self.shared.bridge.clear_exception();
        if code == ErrorCode::FrameFormatUnsupported
            || !(settings.anime4k_on || settings.sharpen_on)
        {
            self.error(code, detail);
            return;
        }
        self.failed = Some((settings, code, detail.clone()));
        self.dirty = true;
        self.phase_for(settings, EnhancementPhase::Error, Some(code), detail);
    }
    fn error(&mut self, code: ErrorCode, detail: String) {
        if self.terminal {
            return;
        }
        self.terminal = true;
        self.renderer.take();
        if self.current()
            && let Some(source) = &self.props.source
        {
            if let Err(error) = self.shared.bridge.direct_holding(self.generation, true) {
                log::warn!("[Android video] audio hold after failure was rejected: {error}");
            }
            self.shared.bridge.event(&VideoEvent::Error {
                source_id: source.id.clone(),
                code,
                detail,
            });
        }
    }
    fn phase(&mut self, phase: EnhancementPhase, code: Option<ErrorCode>, detail: String) {
        self.phase_for(self.props.enhancement, phase, code, detail);
    }
    fn phase_for(
        &mut self,
        settings: Enhancement,
        phase: EnhancementPhase,
        code: Option<ErrorCode>,
        detail: String,
    ) {
        let key = (settings, phase, code, detail.clone());
        if self.status.as_ref() == Some(&key) {
            return;
        }
        self.status = Some(key);
        if self.current()
            && let Some(source) = &self.props.source
        {
            self.shared.bridge.event(&VideoEvent::EnhancementStatus {
                source_id: source.id.clone(),
                settings,
                phase,
                code,
                detail,
            });
        }
    }
}
