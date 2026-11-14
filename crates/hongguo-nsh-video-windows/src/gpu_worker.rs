use crate::{
    GpuRenderer, VideoError,
    state::{Picture, Shared},
};
use hongguo_enhancement::FrameBudget;
use hongguo_nsh_video_api::{Enhancement, EnhancementPhase, ErrorCode, VideoEvent};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

pub(crate) fn run(shared: Arc<Shared>) {
    let mut renderer: Option<GpuRenderer> = None;
    let mut retired = Vec::new();
    let mut source = None;
    let mut dimensions = None;
    let mut last = None;
    let mut failed: Option<Enhancement> = None;
    let mut budget = FrameBudget::default();
    let mut mode = (false, false);
    let mut phase = None;
    let mut active = false;
    loop {
        retired.retain_mut(|renderer: &mut GpuRenderer| {
            if let Err(error) = renderer.poll() {
                log::debug!("retiring enhancement work: {error}");
            }
            renderer.busy()
        });
        if shared.stopped.load(Ordering::Acquire) {
            retire(&mut renderer, &mut retired);
            if retired.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }
        if !shared.consumer_active.load(Ordering::Acquire) {
            retire(&mut renderer, &mut retired);
            shared
                .picture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let props = shared
                .props
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _waited = shared.wake_gpu.wait_timeout(
                props,
                if retired.is_empty() {
                    Duration::from_secs(1)
                } else {
                    Duration::from_millis(16)
                },
            );
            continue;
        }
        let (props, revision) = {
            let props = shared
                .props
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (props.clone(), shared.revision.load(Ordering::Acquire))
        };
        if source != props.source {
            source = props.source.clone();
            retire(&mut renderer, &mut retired);
            dimensions = None;
            last = None;
            failed = None;
            budget = FrameBudget::default();
            phase = None;
        }
        let next_mode = (props.enhancement.anime4k_on, props.enhancement.sharpen_on);
        let changed_mode = next_mode != mode;
        let changed_active = props.active != active;
        if changed_active && props.active {
            failed = None;
        }
        active = props.active;
        if next_mode != mode {
            mode = next_mode;
            budget = FrameBudget::default();
        }
        let source_id = source
            .as_ref()
            .map(|source| source.id.clone())
            .unwrap_or_default();
        if !mode.0 && !mode.1 {
            retire(&mut renderer, &mut retired);
            failed = None;
            last = None;
            set_phase(
                &shared,
                &source_id,
                &mut phase,
                props.enhancement,
                EnhancementPhase::Idle,
                None,
            );
        } else if !props.active || shared.video_gap.load(Ordering::Acquire) {
            retire(&mut renderer, &mut retired);
            last = None;
            shared
                .picture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            set_phase(
                &shared,
                &source_id,
                &mut phase,
                props.enhancement,
                EnhancementPhase::Idle,
                None,
            );
        } else if failed != Some(props.enhancement) {
            if (changed_mode || changed_active || renderer.is_none())
                && shared
                    .raw
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some()
            {
                set_phase(
                    &shared,
                    &source_id,
                    &mut phase,
                    props.enhancement,
                    EnhancementPhase::Loading,
                    None,
                );
            }
            let draw_error = shared
                .gpu_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let result = match draw_error {
                Some(error) => Err(error),
                None => step(
                    &shared,
                    revision,
                    props.enhancement,
                    &mut renderer,
                    &mut dimensions,
                    &mut last,
                    &mut budget,
                ),
            };
            match result {
                Ok(Some(EnhancementPhase::Ready)) => set_phase(
                    &shared,
                    &source_id,
                    &mut phase,
                    props.enhancement,
                    EnhancementPhase::Ready,
                    None,
                ),
                Ok(Some(EnhancementPhase::Loading)) => set_phase(
                    &shared,
                    &source_id,
                    &mut phase,
                    props.enhancement,
                    EnhancementPhase::Loading,
                    None,
                ),
                Ok(_) => {}
                Err(error) => {
                    retire(&mut renderer, &mut retired);
                    failed = Some(props.enhancement);
                    last = None;
                    shared
                        .picture
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take();
                    set_phase(
                        &shared,
                        &source_id,
                        &mut phase,
                        props.enhancement,
                        EnhancementPhase::Error,
                        Some(&error),
                    );
                }
            }
        }
        let props = shared
            .props
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let idle = !props.active
            || shared.video_gap.load(Ordering::Acquire)
            || !(props.enhancement.anime4k_on || props.enhancement.sharpen_on)
            || props.paused && renderer.as_ref().is_none_or(|r| !r.busy());
        if !shared.stopped.load(Ordering::Acquire) {
            let _waited = shared.wake_gpu.wait_timeout(
                props,
                if idle && retired.is_empty() {
                    Duration::from_secs(1)
                } else {
                    Duration::from_millis(16)
                },
            );
        }
    }
}

fn retire(renderer: &mut Option<GpuRenderer>, retired: &mut Vec<GpuRenderer>) {
    if let Some(renderer) = renderer.take() {
        retired.push(renderer);
    }
}

fn step(
    shared: &Arc<Shared>,
    revision: u64,
    options: Enhancement,
    renderer: &mut Option<GpuRenderer>,
    dimensions: &mut Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11Device,
        u32,
        u32,
        nsh_platform_windows::OutputColorMode,
    )>,
    last: &mut Option<(u64, u64, u64)>,
    budget: &mut FrameBudget,
) -> Result<Option<EnhancementPhase>, VideoError> {
    if let Some(renderer) = renderer.as_ref()
        && let Some(completed) = renderer.poll()?
        && shared.valid(completed.epoch())
    {
        if budget.observe(completed.elapsed()) {
            return Err(VideoError::new(
                ErrorCode::EnhancementSlow,
                "GPU enhancement exceeded consecutive frame budget",
            ));
        }
        *shared
            .picture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Picture {
            revision: completed.epoch(),
            frame: completed.frame().clone(),
        });
        shared.context.redraw();
        return Ok(Some(EnhancementPhase::Ready));
    }
    let snapshot = shared
        .raw
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|raw| (raw.device.clone(), raw.metadata, raw.output_mode));
    let Some((device, metadata, mode)) = snapshot else {
        return Ok(None);
    };
    let size = (metadata.width, metadata.height);
    let mut loading = false;
    if dimensions
        .as_ref()
        .is_none_or(|(old, width, height, old_mode)| {
            old != &device || (*width, *height) != size || old_mode != &mode
        })
        || renderer.is_none()
    {
        *renderer = Some(pollster::block_on(GpuRenderer::new(
            &device, size.0, size.1, mode,
        ))?);
        *dimensions = Some((device, size.0, size.1, mode));
        *last = None;
        loading = true;
    }
    if !shared.valid(revision) || *last == Some((revision, metadata.generation, metadata.serial)) {
        return Ok(None);
    }
    let renderer = renderer
        .as_mut()
        .expect("initialized for current frame dimensions");
    let captured = {
        // Snapshot PTS and pixels are copied together. This lock is released before any shader future.
        let raw = shared
            .raw
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match raw.as_ref() {
            Some(raw) => renderer
                .capture(&raw.texture, raw.metadata)?
                .map(|input| (input, raw.color.clone())),
            None => None,
        }
    };
    if let Some((input, color)) = captured {
        let metadata = input.metadata();
        if pollster::block_on(
            renderer.render(input, color, options, revision, || shared.valid(revision)),
        )? {
            *last = Some((revision, metadata.generation, metadata.serial));
        }
    }
    Ok(loading.then_some(EnhancementPhase::Loading))
}

fn set_phase(
    shared: &Shared,
    source_id: &str,
    previous: &mut Option<(EnhancementPhase, Enhancement, Option<ErrorCode>, String)>,
    settings: Enhancement,
    phase: EnhancementPhase,
    error: Option<&VideoError>,
) {
    let code = error.map(VideoError::code);
    let detail = error
        .map(|error| error.detail().to_owned())
        .unwrap_or_default();
    if previous.as_ref() == Some(&(phase, settings, code, detail.clone())) {
        return;
    }
    *previous = Some((phase, settings, code, detail.clone()));
    shared.emit(VideoEvent::EnhancementStatus {
        source_id: source_id.to_owned(),
        settings,
        phase,
        code,
        detail,
    });
}
