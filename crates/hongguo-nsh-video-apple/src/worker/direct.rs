use super::*;
use hongguo_nsh_video_api::frames::{FrameLease, PixelFrame};

pub(super) struct Direct {
    pub frames: Arc<FrameLease>,
    pub pixels: Option<Arc<PixelFrame>>,
    pub native: Option<Arc<hongguo_media_types::PresentationFrame>>,
    pub submitted: Option<(u64, u64)>,
    generation: Option<u64>,
    waiting_presentation: bool,
}

impl Direct {
    pub(super) fn blank(&mut self) {
        self.native = None;
        self.pixels = None;
        self.submitted = None;
        self.waiting_presentation = false;
    }
    pub fn open(frames: Arc<FrameLease>) -> Result<Self, String> {
        Ok(Self {
            frames,
            pixels: None,
            native: None,
            submitted: None,
            generation: None,
            waiting_presentation: true,
        })
    }

    pub fn seek(&mut self, position_ms: u64, applied: Option<u64>) -> Result<bool, String> {
        let generation = match applied {
            Some(generation) => generation,
            None => {
                let Some(generation) = self.frames.seek_if_owner(position_ms)? else {
                    return Ok(false);
                };
                generation
            }
        };
        self.generation = Some(generation);
        self.pixels = None;
        self.native = None;
        self.submitted = None;
        self.waiting_presentation = true;
        Ok(true)
    }
}

impl Worker {
    pub(super) fn direct_failure(&mut self, error: OutputError) {
        if self.shared.stop.load(Ordering::Acquire) {
            return;
        }
        if let Some(video) = &self.video {
            video.control().pause();
        }
        if error.is_decoder_surface()
            && let Some(direct) = &mut self.direct
        {
            match direct.frames.reject_decoder_if_owner(error.detail()) {
                Ok(true) => {
                    direct.generation = None;
                    direct.native = None;
                    direct.pixels = None;
                    direct.submitted = None;
                    direct.waiting_presentation = true;
                    self.frame.take();
                    self.dirty = true;
                    return;
                }
                Ok(false) => {
                    self.abandon_consumer();
                    return;
                }
                Err(detail) => {
                    self.terminal_failure(OutputError::new(ErrorCode::DecoderFailed, detail));
                    return;
                }
            }
        }
        const MAX_GPU_REBUILDS: u8 = 2;
        if matches!(
            error.code(),
            ErrorCode::GpuFailed | ErrorCode::GpuUnavailable
        ) && self.gpu_rebuilds < MAX_GPU_REBUILDS
        {
            self.gpu_rebuilds += 1;
            self.renderer.take();
            self.frame.take();
            self.dirty = true;
            if let Some(direct) = &mut self.direct {
                direct.generation = None;
                direct.native = None;
                direct.pixels = None;
                direct.submitted = None;
                direct.waiting_presentation = true;
                match direct.frames.recover_presentation_if_owner(error.detail()) {
                    Ok(true) => {}
                    Ok(false) => self.abandon_consumer(),
                    Err(detail) => {
                        self.terminal_failure(OutputError::new(ErrorCode::DecoderFailed, detail))
                    }
                }
            }
            return;
        }
        self.terminal_failure(error);
    }

    fn terminal_failure(&mut self, error: OutputError) {
        self.status(EnhancementPhase::Error, Some(&error));
        self.event(VideoEvent::Error {
            source_id: self.source_id(),
            code: error.code(),
            detail: error.detail().into(),
        });
        self.abandon_consumer();
    }

    fn abandon_consumer(&mut self) {
        self.video.take();
        self.direct.take();
        self.pending_seek.take();
        self.renderer.take();
        self.frame.take();
        self.dirty = false;
        self.layers.set_visible(false);
    }

    pub(super) fn read_direct(&mut self) {
        let result = (|| {
            let video = self.video.as_ref().expect("active audio");
            if !video.ready()? {
                return Ok(None);
            }
            let direct = self.direct.as_ref().expect("direct source");
            let info = direct.frames.info().map_err(decoder_error)?;
            let frame = direct
                .frames
                .frame_at(video.position_ms().unwrap_or(0))
                .map_err(decoder_error)?;
            let lead = if self.props.paused || direct.frames.holding().unwrap_or(true) {
                0.0
            } else {
                crate::display_clock::presentation_lead(f64::from_bits(
                    self.shared.display_deadline.load(Ordering::Acquire),
                )) * self.props.rate
            };
            let native = direct
                .frames
                .native_frame_at(video.position_ms().unwrap_or(0) as f64 / 1000.0 + lead)
                .map_err(decoder_error)?;
            Ok::<_, OutputError>(info.map(|info| (info, frame, native, video.duration_ms())))
        })();
        match result {
            Ok(Some((info, frame, native, duration_ms))) => {
                let (width, height) = native
                    .as_ref()
                    .map(|frame| display_dimensions(&frame.image.descriptor()))
                    .unwrap_or((info.width, info.height));
                let metadata = (duration_ms, width, height);
                if self.last_metadata != Some(metadata) {
                    self.last_metadata = Some(metadata);
                    self.event(VideoEvent::Load {
                        source_id: self.source_id(),
                        duration_ms,
                        width,
                        height,
                    });
                }
                let direct = self.direct.as_mut().expect("direct source");
                if let Some(native) = native
                    && direct
                        .generation
                        .is_none_or(|generation| generation <= native.generation)
                    && direct.native.as_ref().is_none_or(|previous| {
                        previous.serial != native.serial || previous.generation != native.generation
                    })
                {
                    direct.generation = Some(native.generation);
                    if native.duration_secs > 0.0 {
                        self.shared.preferred_frame_rate.store(
                            (self.props.rate / native.duration_secs).ceil() as u32,
                            Ordering::Release,
                        );
                    }
                    direct.native = Some(native);
                    direct.pixels = None;
                    self.frame.take();
                    self.dirty = true;
                }
                if let Some(frame) = frame
                    && direct
                        .generation
                        .is_none_or(|generation| generation <= frame.generation)
                    && direct.pixels.as_ref().is_none_or(|previous| {
                        previous.serial != frame.serial || previous.generation != frame.generation
                    })
                {
                    direct.generation = Some(frame.generation);
                    direct.pixels = Some(frame);
                    self.frame.take();
                    self.dirty = true;
                }
                self.release_direct_audio();
            }
            Ok(None) => {}
            Err(error) => self.direct_failure(error),
        }
    }

    pub(super) fn prepare_direct(&mut self) {
        if self
            .renderer
            .as_ref()
            .is_some_and(|renderer| !renderer.idle())
        {
            return;
        }
        if self.frame.is_some() {
            return;
        }
        let Some(direct) = self.direct.as_ref() else {
            return;
        };
        let native = direct.native.clone();
        let pixels = direct.pixels.clone();
        if native.is_none() && pixels.is_none() {
            return;
        }
        if self.renderer.is_none() {
            match pollster::block_on(Renderer::new(self.layers.metal.clone())) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    self.direct_failure(error);
                    return;
                }
            }
        }
        let renderer = self.renderer.as_mut().expect("renderer initialized");
        if let Some(native) = native {
            let target = self.viewport.color_target();
            match renderer.upload_native(native, target) {
                Ok(frame) => self.frame = Some(frame),
                Err(error) => {
                    self.direct_failure(error);
                    return;
                }
            }
        } else if let Some(frame) = pixels {
            self.frame = Some(renderer.upload(&frame));
        }
        self.dirty = true;
    }

    pub(super) fn present_direct(&mut self) {
        let direct = self.direct.as_mut().expect("direct source");
        let Some((generation, serial)) = direct.submitted.take() else {
            return;
        };
        if direct
            .generation
            .is_some_and(|expected| generation != expected)
        {
            return;
        }
        match direct.frames.presented_if_owner(generation, serial) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.direct_failure(decoder_error(error));
                return;
            }
        }
        direct.waiting_presentation = false;
        self.release_direct_audio();
    }

    fn release_direct_audio(&self) {
        let Some(direct) = &self.direct else {
            return;
        };
        let Some(video) = &self.video else {
            return;
        };
        if !direct.waiting_presentation
            && direct.frames.holding() == Ok(false)
            && video.ready().unwrap_or(false)
            && video.hold(false)
        {
            self.shared.reconcile_playback(&self.source_id());
        }
    }

    pub(super) fn confirm_direct_seek(&mut self) {
        let Some((request, started)) = &self.pending_seek else {
            return;
        };
        if started.elapsed() >= Duration::from_secs(15) {
            self.direct_failure(OutputError::new(
                ErrorCode::SeekFailed,
                "native session did not present the target seek frame",
            ));
            self.pending_seek = None;
            return;
        }
        let direct = self.direct.as_mut().expect("direct source");
        match direct.frames.holding() {
            Ok(false) if !direct.waiting_presentation => {
                if let Some(position_ms) = self.video.as_ref().and_then(AudioOutput::position_ms) {
                    self.event(VideoEvent::Seeked {
                        source_id: request.source_id.clone(),
                        request_id: request.request_id.clone(),
                        position_ms,
                    });
                    self.pending_seek = None;
                    self.release_direct_audio();
                }
            }
            Ok(true) | Ok(false) => {}
            Err(error) => self.direct_failure(decoder_error(error)),
        }
    }
}

fn decoder_error(error: String) -> OutputError {
    OutputError::new(ErrorCode::DecoderFailed, error)
}

fn display_dimensions(frame: &hongguo_media_types::FrameDescriptor) -> (u32, u32) {
    let (numerator, denominator) = frame
        .color
        .sample_aspect_ratio
        .filter(|(n, d)| *n > 0 && *d > 0)
        .unwrap_or((1, 1));
    let width = (frame.crop.width as f64 * f64::from(numerator) / f64::from(denominator))
        .round()
        .max(1.0) as u32;
    let height = frame.crop.height.max(1).min(u32::MAX as usize) as u32;
    if frame.rotation_degrees % 180 == 90 {
        (height, width)
    } else {
        (width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn load_dimensions_follow_visible_crop_sar_and_rotation() {
        let mut descriptor = hongguo_media_types::FrameDescriptor {
            width: 8,
            height: 8,
            bit_depth: 10,
            crop: hongguo_media_types::CropRect {
                x: 1,
                y: 1,
                width: 3,
                height: 5,
            },
            rotation_degrees: 90,
            color: hongguo_media_types::ColorMetadata {
                sample_aspect_ratio: Some((2, 1)),
                ..Default::default()
            },
        };
        assert_eq!(display_dimensions(&descriptor), (5, 6));
        descriptor.rotation_degrees = 0;
        assert_eq!(display_dimensions(&descriptor), (6, 5));
    }
}
