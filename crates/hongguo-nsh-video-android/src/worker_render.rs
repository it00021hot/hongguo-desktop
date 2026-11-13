use super::Worker;
use crate::render::Renderer;
use hongguo_nsh_video_api::{EnhancementPhase, ErrorCode};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

impl Worker {
    pub(super) fn prepare_renderer(&mut self, size: Option<(u32, u32)>) -> bool {
        if !self.enabled() {
            self.phase(EnhancementPhase::Idle, None, String::new());
            return false;
        }
        if let Some((settings, code, detail)) = self
            .failed
            .clone()
            .filter(|(settings, _, _)| *settings == self.props.enhancement)
        {
            self.phase_for(settings, EnhancementPhase::Error, Some(code), detail);
            return false;
        }
        if !self.requested_output {
            self.phase(EnhancementPhase::Loading, None, String::new());
            match self.shared.bridge.prepare_output(self.generation) {
                Ok(()) => self.requested_output = true,
                Err(error) => {
                    self.fallback(ErrorCode::GpuUnavailable, error);
                    return false;
                }
            }
        }
        if self.renderer.is_none() {
            let (Some(window), Some(size)) = (self.viewport.window.clone(), size) else {
                return false;
            };
            if self.viewport.width == 0 || self.viewport.height == 0 {
                return false;
            }
            self.phase(EnhancementPhase::Loading, None, String::new());
            let result = pollster::block_on(Renderer::new(
                &self.shared.bridge,
                window,
                (self.viewport.width, self.viewport.height),
                size,
                self.props.enhancement,
            ));
            match result {
                Ok(renderer)
                    if self.current()
                        && self.shared.revision.load(Ordering::Acquire) == self.revision =>
                {
                    self.output_token = self.output_token.wrapping_add(1);
                    self.renderer_generation = self.generation;
                    self.renderer = Some(renderer);
                    let renderer = self
                        .renderer
                        .as_ref()
                        .expect("retained before decoder attachment");
                    if let Err(error) = self.shared.bridge.output(
                        self.generation,
                        self.output_token,
                        Some((renderer.frames.surface(), renderer.frames.surface_texture())),
                    ) {
                        self.fallback(ErrorCode::FrameUnavailable, error);
                        return false;
                    }
                    self.first_frame = Some(Instant::now());
                }
                Ok(_) => return false,
                Err((code, error)) => {
                    self.fallback(code, error);
                    return false;
                }
            }
        }
        true
    }
    pub(super) fn draw(&mut self) {
        let completed = self.renderer.as_mut().expect("renderer initialized").poll();
        match completed {
            Ok(Some(completed)) if self.current() => {
                if self.budget.observe(completed.elapsed) {
                    self.fallback_for(
                        completed.settings,
                        ErrorCode::EnhancementSlow,
                        "GPU enhancement exceeded consecutive frame budget".into(),
                    );
                    return;
                }
                if completed.presented
                    && completed.settings == self.props.enhancement
                    && self.shared.revision.load(Ordering::Acquire) == self.revision
                {
                    if let Err(error) = self.shared.bridge.show(self.generation, true) {
                        self.fallback(ErrorCode::GpuFailed, error);
                        return;
                    }
                    self.first_frame = None;
                    self.phase(EnhancementPhase::Ready, None, String::new());
                }
            }
            Err((code, error)) => {
                let settings = self
                    .renderer
                    .as_ref()
                    .expect("renderer initialized")
                    .settings();
                self.fallback_for(settings, code, error);
                return;
            }
            _ => {}
        }
        if self
            .first_frame
            .is_some_and(|started| started.elapsed() > Duration::from_secs(3))
        {
            self.fallback(
                ErrorCode::FrameUnavailable,
                "The decoder did not supply a frame to the enhancement output".into(),
            );
            return;
        }
        let renderer = self.renderer.as_mut().expect("renderer initialized");
        if !renderer.ready() {
            return;
        }
        // Consume one notification only once input/GPU are writable. A duplicate
        // timestamp must not leave updateTexImage spinning, and an event that
        // arrives during the submission must remain queued for the next frame.
        let fresh = self.shared.frame.swap(false, Ordering::AcqRel);
        let resized = match renderer
            .surface
            .resize(self.viewport.width, self.viewport.height)
        {
            Ok(resized) => resized,
            Err(error) => {
                self.fallback(ErrorCode::GpuFailed, error);
                return;
            }
        };
        match pollster::block_on(renderer.draw(
            fresh,
            self.dirty,
            self.props.enhancement,
            self.props.resize_mode,
        )) {
            Ok(true) => {
                // EGL's first swap after resize can retire the old-size back
                // buffer. Present this same real image once more while paused.
                self.dirty = resized;
            }
            Ok(false) => {}
            Err((code, error)) => self.fallback(code, error),
        }
    }
}
