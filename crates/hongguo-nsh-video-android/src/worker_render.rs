use super::Worker;
use crate::render::{RenderFrame, Renderer};
use hongguo_nsh_video_api::{EnhancementPhase, ErrorCode};
use std::sync::atomic::Ordering;

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
            let (Some(_), Some(size)) = (self.viewport.window.as_ref(), size) else {
                return false;
            };
            if self.viewport.width == 0 || self.viewport.height == 0 {
                return false;
            }
            self.phase(EnhancementPhase::Loading, None, String::new());
            let Some(lease) = self.direct_lease() else {
                return false;
            };
            let result = pollster::block_on(Renderer::new(
                &self.viewport,
                size,
                self.options(),
                self.shared.bridge.clone(),
                &lease,
                self.generation,
            ));
            match result {
                Ok(renderer)
                    if self.current()
                        && self.shared.revision.load(Ordering::Acquire) == self.revision =>
                {
                    self.renderer = Some(renderer);
                }
                Ok(_) => return false,
                Err((code, error)) => {
                    if self.viewport.float_output {
                        log::info!(
                            "[Android video] native HDR output unavailable, selecting SDR: {error}"
                        );
                        if let Err(detail) = self.shared.bridge.sdr_output(self.generation) {
                            self.error(code, detail);
                        }
                        return false;
                    }
                    self.fallback(code, error);
                    return false;
                }
            }
        }
        let Some(lease) = self.direct_lease() else {
            return false;
        };
        match self
            .renderer
            .as_mut()
            .expect("renderer initialized")
            .bind_if_needed(&lease)
        {
            Ok(true) => {
                if let Some((had_surface, detail)) = &self.gpu_recovery
                    && !had_surface
                    && !self
                        .renderer
                        .as_ref()
                        .expect("renderer initialized")
                        .has_decoder_surface()
                {
                    match lease.recover_presentation_if_owner(detail) {
                        Ok(true) => {}
                        Ok(false) => return false,
                        Err(error) => {
                            self.error(ErrorCode::GpuFailed, error);
                            return false;
                        }
                    }
                }
                self.gpu_recovery = None;
                true
            }
            Ok(false) => false,
            Err((code, error)) => {
                self.error(code, error);
                false
            }
        }
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
                    && self.shared.revision.load(Ordering::Acquire) == self.revision
                {
                    if let Err(error) = self.shared.bridge.show(self.generation, true) {
                        self.fallback(ErrorCode::GpuFailed, error);
                        return;
                    }
                    // GPU completion only retires image leases. Java acknowledges
                    // the exact HAL swap token when TextureView consumes that image.
                    if let Err(error) = self.shared.bridge.direct_presented(
                        self.generation,
                        &completed.frame,
                        completed.token,
                        completed.swap_succeeded,
                        completed.output_version,
                    ) {
                        self.error(ErrorCode::GpuFailed, error);
                        return;
                    }
                    if self.failed.is_none() && completed.settings == self.props.enhancement {
                        self.phase(EnhancementPhase::Ready, None, String::new());
                    }
                }
            }
            Err((code, error)) => {
                let had_surface = self
                    .renderer
                    .as_ref()
                    .expect("renderer initialized")
                    .has_decoder_surface();
                if let Err(detail) = self.shared.bridge.direct_holding(self.generation, true) {
                    self.error(code, detail);
                    return;
                }
                self.renderer.take();
                self.gpu_recovery = Some((had_surface, error));
                self.dirty = true;
                self.phase(EnhancementPhase::Loading, None, String::new());
                self.shared.wake();
                return;
            }
            _ => {}
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
        let Some(lease) = self.direct_lease() else {
            return;
        };
        let pixel = {
            let position = match lease.position() {
                Ok(Some(position)) if position.video_gap => {
                    self.renderer
                        .as_mut()
                        .expect("renderer initialized")
                        .clear_media();
                    if !self.video_gap
                        && let Err(error) = self
                            .shared
                            .bridge
                            .show(self.generation, false)
                            .and_then(|()| {
                                self.shared
                                    .bridge
                                    .direct_gap(self.generation, self.video_size.unwrap_or((0, 0)))
                            })
                    {
                        self.error(ErrorCode::GpuFailed, error);
                    }
                    self.video_gap = true;
                    return;
                }
                Ok(Some(position)) => {
                    if self.video_gap {
                        self.dirty = true;
                        self.video_gap = false;
                    }
                    position.position_secs
                }
                Ok(None) => 0.0,
                Err(error) => {
                    self.error(ErrorCode::DecoderFailed, error);
                    return;
                }
            };
            let frame = match lease.native_frame_at(position) {
                Ok(Some(frame)) => Some(RenderFrame::Native(frame)),
                Ok(None) => match lease.frame_at((position.max(0.0) * 1000.0).round() as u64) {
                    Ok(frame) => frame.map(RenderFrame::Preview),
                    Err(error) => {
                        self.error(ErrorCode::DecoderFailed, error);
                        return;
                    }
                },
                Err(error) => {
                    self.error(ErrorCode::DecoderFailed, error);
                    return;
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
        };
        let renderer = self.renderer.as_mut().expect("renderer initialized");
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
            self.dirty,
            options,
            self.props.resize_mode,
            pixel,
            &lease,
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
