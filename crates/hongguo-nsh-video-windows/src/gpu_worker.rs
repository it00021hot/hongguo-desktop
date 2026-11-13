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
    let mut source = None;
    let mut dimensions = None;
    let mut last = None;
    let mut failed: Option<Enhancement> = None;
    let mut budget = FrameBudget::default();
    let mut mode = (false, false);
    let mut phase = None;
    let mut active = false;
    while !shared.stopped.load(Ordering::Acquire) {
        let (props, revision) = {
            let props = shared
                .props
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (props.clone(), shared.revision.load(Ordering::Acquire))
        };
        if source != props.source {
            source = props.source.clone();
            renderer = None;
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
            renderer = None;
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
        } else if !props.active {
            renderer = None;
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
                    renderer = None;
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
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn step(
    shared: &Arc<Shared>,
    revision: u64,
    options: Enhancement,
    renderer: &mut Option<GpuRenderer>,
    dimensions: &mut Option<(u32, u32)>,
    last: &mut Option<(u64, u64)>,
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
        .map(|raw| (raw.device.clone(), raw.metadata));
    let Some((device, metadata)) = snapshot else {
        return Ok(None);
    };
    let size = (metadata.width, metadata.height);
    let mut loading = false;
    if *dimensions != Some(size) || renderer.is_none() {
        *renderer = Some(pollster::block_on(GpuRenderer::new(
            &device, size.0, size.1,
        ))?);
        *dimensions = Some(size);
        *last = None;
        loading = true;
    }
    if !shared.valid(revision) || *last == Some((revision, metadata.pts_ms)) {
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
            Some(raw) => renderer.capture(&raw.texture, raw.metadata)?,
            None => None,
        }
    };
    if let Some(input) = captured {
        let pts = input.metadata().pts_ms;
        if pollster::block_on(renderer.render(input, options, revision, || shared.valid(revision)))?
        {
            *last = Some((revision, pts));
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
