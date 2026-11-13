use super::*;
use hongguo_nsh_video_api::frames::{FrameLease, PixelFrame, open_frames};

pub(super) struct Direct {
    pub frames: FrameLease,
    pub pixels: Option<Arc<PixelFrame>>,
    pub submitted: Option<(u64, u64)>,
    generation: Option<u64>,
    audio_seek_done: bool,
}

impl Direct {
    pub fn open(url: &str) -> Result<Self, String> {
        Ok(Self {
            frames: open_frames(url)?,
            pixels: None,
            submitted: None,
            generation: None,
            audio_seek_done: false,
        })
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        self.generation = Some(self.frames.seek(position_ms)?);
        self.pixels = None;
        self.submitted = None;
        self.audio_seek_done = false;
        Ok(())
    }
}

impl Worker {
    pub(super) fn direct_failure(&mut self, error: OutputError) {
        if self.enhanced()
            && self.failed != Some(self.props.enhancement)
            && matches!(
                error.code(),
                ErrorCode::GpuFailed
                    | ErrorCode::GpuUnavailable
                    | ErrorCode::InputTooLarge
                    | ErrorCode::EnhancementSlow
            )
        {
            self.failed = Some(self.props.enhancement);
            self.status(EnhancementPhase::Error, Some(&error));
            self.renderer.take();
            self.frame.take();
            self.dirty = true;
        } else {
            if let Some(video) = &self.video {
                video.hold(true);
            }
            self.event(VideoEvent::Error {
                source_id: self.source_id(),
                code: error.code(),
                detail: error.detail().into(),
            });
            self.video.take();
            self.direct.take();
            self.pending_seek.take();
            self.renderer.take();
            self.frame.take();
            self.dirty = false;
            self.layers.set_visible(false);
        }
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
            Ok::<_, OutputError>(info.map(|info| (info, frame, video.duration_ms())))
        })();
        match result {
            Ok(Some((info, frame, duration_ms))) => {
                let metadata = (duration_ms, info.width, info.height);
                if self.last_metadata != Some(metadata) {
                    self.last_metadata = Some(metadata);
                    self.event(VideoEvent::Load {
                        source_id: self.source_id(),
                        duration_ms,
                        width: info.width,
                        height: info.height,
                    });
                }
                let direct = self.direct.as_mut().expect("direct source");
                if let Some(frame) = frame
                    && direct
                        .generation
                        .is_none_or(|generation| generation == frame.generation)
                    && direct.pixels.as_ref().is_none_or(|previous| {
                        previous.serial != frame.serial || previous.generation != frame.generation
                    })
                {
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
        if self.frame.is_some() {
            return;
        }
        let Some(frame) = self
            .direct
            .as_ref()
            .and_then(|direct| direct.pixels.clone())
        else {
            return;
        };
        if self.renderer.is_none() {
            match pollster::block_on(Renderer::new(self.layers.metal.clone())) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    self.direct_failure(error);
                    return;
                }
            }
        }
        self.frame = self
            .renderer
            .as_ref()
            .map(|renderer| renderer.upload(&frame));
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
        if let Err(error) = direct.frames.presented(generation, serial) {
            self.direct_failure(decoder_error(error));
            return;
        }
        self.release_direct_audio();
    }

    fn release_direct_audio(&self) {
        let Some(direct) = &self.direct else {
            return;
        };
        let Some(video) = &self.video else {
            return;
        };
        if direct.frames.holding() == Ok(false)
            && (self.pending_seek.is_none() || direct.audio_seek_done)
            && video.ready().unwrap_or(false)
            && video.hold(false)
        {
            self.shared.reconcile_playback(&self.source_id());
        }
    }

    pub(super) fn confirm_direct_seek(&mut self) {
        let Some((request, completion, started)) = &self.pending_seek else {
            return;
        };
        let direct = self.direct.as_mut().expect("direct source");
        if !direct.audio_seek_done {
            match completion.try_recv() {
                Ok(true) => direct.audio_seek_done = true,
                Err(TryRecvError::Empty) if started.elapsed() < Duration::from_secs(15) => return,
                _ => {
                    self.direct_failure(OutputError::new(
                        ErrorCode::SeekFailed,
                        "AVPlayer did not finish the current seek",
                    ));
                    self.pending_seek = None;
                    return;
                }
            }
        }
        match direct.frames.holding() {
            Ok(false) => {
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
            Ok(true) => {}
            Err(error) => self.direct_failure(decoder_error(error)),
        }
    }
}

fn decoder_error(error: String) -> OutputError {
    OutputError::new(ErrorCode::DecoderFailed, error)
}
