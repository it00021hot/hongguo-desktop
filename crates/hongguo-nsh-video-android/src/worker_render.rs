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
            if !self.direct() {
                return false;
            }
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
                self.options(),
                self.direct(),
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
                    if let Some(frames) = &renderer.frames
                        && let Err(error) = self.shared.bridge.output(
                            self.generation,
                            self.output_token,
                            Some((frames.surface(), frames.surface_texture())),
                        )
                    {
                        self.fallback(ErrorCode::FrameUnavailable, error);
                        return false;
                    }
                    if !self.direct() {
                        self.first_frame = Some(Instant::now());
                    }
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
                if self.failed.is_none()
                    && (completed.settings.anime4k_on || completed.settings.sharpen_on)
                    && self.budget.observe(completed.elapsed)
                {
                    self.fallback_for(
                        completed.settings,
                        ErrorCode::EnhancementSlow,
                        "GPU enhancement exceeded consecutive frame budget".into(),
                    );
                    return;
                }
                if completed.presented
                    && (self.direct() || completed.settings == self.props.enhancement)
                    && self.shared.revision.load(Ordering::Acquire) == self.revision
                {
                    if let (Some(frame), Some(lease)) = (&completed.frame, self.direct_lease()) {
                        if self.shared.audio_seeking.load(Ordering::Acquire) {
                            return;
                        }
                        match lease.presented(frame.generation, frame.serial) {
                            Ok(true) => {
                                // Queue visibility before the callback can release
                                // audio on the main handler.
                                if let Err(error) = self.shared.bridge.show(self.generation, true) {
                                    self.fallback(ErrorCode::GpuFailed, error);
                                    return;
                                }
                                if let Err(error) =
                                    self.shared.bridge.direct_presented(self.generation, frame)
                                {
                                    self.error(ErrorCode::GpuFailed, error);
                                    return;
                                }
                            }
                            // presented is a one-shot audio release. A viewport
                            // rebuild must also show the already released image.
                            Ok(false) => match lease.holding() {
                                Ok(false) => {}
                                Ok(true) => return,
                                Err(error) => {
                                    self.error(ErrorCode::DecoderFailed, error);
                                    return;
                                }
                            },
                            Err(error) => {
                                self.error(ErrorCode::DecoderFailed, error);
                                return;
                            }
                        }
                    }
                    if let Err(error) = self.shared.bridge.show(self.generation, true) {
                        self.fallback(ErrorCode::GpuFailed, error);
                        return;
                    }
                    self.first_frame = None;
                    if self.failed.is_none() {
                        self.phase(EnhancementPhase::Ready, None, String::new());
                    }
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
        let options = self.options();
        if !self
            .renderer
            .as_ref()
            .expect("renderer initialized")
            .ready()
        {
            return;
        }
        let pixel = if let Some(lease) = self.direct_lease() {
            let frame = if self.shared.audio_seeking.load(Ordering::Acquire) {
                None
            } else {
                match lease.frame_at(self.shared.audio_position.load(Ordering::Acquire)) {
                    Ok(frame) => frame,
                    Err(error) => {
                        self.error(ErrorCode::DecoderFailed, error);
                        return;
                    }
                }
            };
            match lease.holding() {
                Ok(holding) if self.holding != Some(holding) => {
                    self.holding = Some(holding);
                    if let Err(error) = self.shared.bridge.direct_holding(self.generation, holding)
                    {
                        self.error(ErrorCode::DecoderFailed, error);
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    self.error(ErrorCode::DecoderFailed, error);
                    return;
                }
            }
            frame
        } else {
            None
        };
        let renderer = self.renderer.as_mut().expect("renderer initialized");
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
            options,
            self.props.resize_mode,
            pixel,
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
