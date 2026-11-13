mod events;
mod picture;

use crate::{
    DecoderEvent, DisplayedFrame, RawFrame, VideoDecoder, VideoError,
    direct::Direct,
    drawing_device,
    state::{RawSnapshot, Shared},
};
use hongguo_nsh_video_api::{ErrorCode, Seek, Source, VideoEvent, VideoProps};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::Graphics::{
    Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext},
    Direct3D11::ID3D11Device,
};

pub(crate) struct Controller {
    source: Option<Source>,
    decoder: Option<VideoDecoder>,
    direct: Option<Direct>,
    device: Option<ID3D11Device>,
    raw: Option<RawFrame>,
    loaded: bool,
    applied: Option<VideoProps>,
    pending: Option<(Seek, Instant)>,
    queued: Option<Seek>,
    bitmap: Option<(Arc<DisplayedFrame>, ID2D1Bitmap1)>,
    progress_at: Instant,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            source: None,
            decoder: None,
            direct: None,
            device: None,
            raw: None,
            loaded: false,
            applied: None,
            pending: None,
            queued: None,
            bitmap: None,
            progress_at: Instant::now(),
        }
    }
}

impl Controller {
    pub fn set_props(&mut self, props: &VideoProps) -> Result<(), VideoError> {
        if self.source != props.source {
            self.decoder.take();
            self.direct.take();
            self.source = None;
            self.raw = None;
            self.loaded = false;
            self.applied = None;
            self.pending = None;
            if self.queued.as_ref().is_some_and(|seek| {
                props
                    .source
                    .as_ref()
                    .is_none_or(|source| source.id != seek.source_id)
            }) {
                self.queued = None;
            }
        } else if let Some(decoder) = &self.decoder {
            if self.loaded && self.direct.as_ref().is_none_or(|direct| direct.audio_ready) {
                let hold = self.hold()?;
                Self::apply(&mut self.applied, decoder, props, hold)?;
            } else if self.direct.is_some() || props.paused || !props.active {
                decoder.pause()?;
            }
        }
        self.bitmap = None;
        Ok(())
    }

    pub fn seek(&mut self, request: Seek, props: &VideoProps) -> Result<(), VideoError> {
        if request.source_id.is_empty() || request.request_id.is_empty() {
            return Err(VideoError::new(
                ErrorCode::InvalidProps,
                "seek requires source and request identities",
            ));
        }
        if props
            .source
            .as_ref()
            .is_none_or(|source| source.id != request.source_id)
        {
            return Ok(());
        }
        self.bitmap = None;
        self.raw = None;
        if self.pending.is_some()
            || !self.loaded
            || self
                .direct
                .as_ref()
                .is_some_and(|direct| !direct.audio_ready)
        {
            self.queued = Some(request);
            return Ok(());
        }
        if let Some(decoder) = &self.decoder {
            if let Some(direct) = &mut self.direct {
                decoder.pause()?;
                direct.seek(request.position_ms)?;
            }
            decoder.seek(request.position_ms)?;
            self.pending = Some((request, Instant::now()));
        } else {
            self.queued = Some(request);
        }
        Ok(())
    }

    pub fn draw(
        &mut self,
        shared: &Arc<Shared>,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
    ) -> Result<(), VideoError> {
        let props = shared
            .props
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if self.source != props.source {
            self.decoder.take();
            self.direct.take();
            self.raw = None;
            self.bitmap = None;
            self.applied = None;
            self.loaded = false;
            self.pending = None;
            shared.clear_frames();
            self.source = props.source.clone();
            let Some(source) = &self.source else {
                return Ok(());
            };
            shared.emit(VideoEvent::LoadStart {
                source_id: source.id.clone(),
            });
            let device = drawing_device(dc)?;
            let handle = source.frame_source.as_deref().ok_or_else(|| {
                VideoError::new(
                    ErrorCode::InvalidProps,
                    "builtin video frame source is required",
                )
            })?;
            self.direct = Some(Direct::open(handle)?);
            let wake = shared.clone();
            self.decoder = Some(VideoDecoder::open(source.clone(), move || {
                wake.context.redraw()
            })?);
            self.device = Some(device);
        }
        let Some(decoder) = self.decoder.take() else {
            return Ok(());
        };
        let result = self.draw_decoder(shared, &decoder, &props, dc, size);
        self.decoder = Some(decoder);
        result
    }

    fn draw_decoder(
        &mut self,
        shared: &Arc<Shared>,
        decoder: &VideoDecoder,
        props: &VideoProps,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
    ) -> Result<(), VideoError> {
        let source_id = decoder.source().id.clone();
        let device = drawing_device(dc)?;
        if self.device.as_ref() != Some(&device) {
            self.device = Some(device);
            self.raw = None;
            self.bitmap = None;
            shared.clear_frames();
            if let Some(direct) = &mut self.direct {
                direct.reset_device();
            }
        }
        if let Some(direct) = &mut self.direct {
            direct.poll_presented()?;
        }
        self.events(shared, decoder)?;
        if let Some(direct) = &self.direct {
            if !self.loaded
                && let Some(info) = direct.info()?
            {
                self.raw = Some(RawFrame::new(dc, info.width, info.height)?);
                self.loaded = true;
                shared.emit(VideoEvent::Load {
                    source_id: source_id.clone(),
                    duration_ms: decoder.duration_ms(),
                    width: info.width,
                    height: info.height,
                });
            }
            if direct.seek_complete() && self.pending.is_some() {
                let (completed, _) = self.pending.take().expect("pending direct seek");
                let position_ms = decoder.position_ms().ok_or_else(|| {
                    VideoError::new(
                        ErrorCode::SeekFailed,
                        "audio seek completed without a finite position",
                    )
                })?;
                shared.emit(VideoEvent::Seeked {
                    source_id: source_id.clone(),
                    request_id: completed.request_id,
                    position_ms,
                });
                self.applied = None;
            }
        }
        if self.loaded {
            if self
                .direct
                .as_ref()
                .is_some_and(|direct| !direct.audio_ready)
            {
                decoder.pause()?;
            } else {
                let hold = self.hold()?;
                Self::apply(&mut self.applied, decoder, props, hold)?;
            }
            if self.pending.is_none()
                && self.direct.as_ref().is_none_or(|direct| direct.audio_ready)
                && let Some(request) = self.queued.take()
            {
                if let Some(direct) = &mut self.direct {
                    decoder.pause()?;
                    direct.seek(request.position_ms)?;
                }
                decoder.seek(request.position_ms)?;
                self.pending = Some((request, Instant::now()));
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|(_, started)| started.elapsed() >= Duration::from_secs(15))
            {
                self.pending = None;
                self.queued = None;
                return Err(VideoError::new(
                    ErrorCode::SeekFailed,
                    "MediaEngine seek completion timed out",
                ));
            }
            if props.active
                && let Some(direct) = &mut self.direct
                && let Some(frame) = direct.frame(decoder.position_ms().unwrap_or(0))?
                && !direct.was_uploaded(&frame)
            {
                if self
                    .raw
                    .as_ref()
                    .and_then(RawFrame::metadata)
                    .is_none_or(|old| (old.width, old.height) != (frame.width, frame.height))
                {
                    self.raw = Some(RawFrame::new(dc, frame.width, frame.height)?);
                }
                let mut snapshot = shared
                    .raw
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let raw = self.raw.as_mut().expect("direct raw frame initialized");
                raw.upload(&frame)?;
                *snapshot = raw.metadata().map(|metadata| RawSnapshot {
                    device: self.device.as_ref().expect("drawing device").clone(),
                    texture: raw.texture().clone(),
                    metadata,
                });
                direct.uploaded(frame);
            }
            if self.progress_at.elapsed()
                >= Duration::from_millis(u64::from(props.progress_interval_ms))
            {
                self.progress_at = Instant::now();
                if let Some(position_ms) = decoder.position_ms() {
                    let duration_ms = decoder.duration_ms();
                    shared.emit(VideoEvent::Progress {
                        source_id: source_id.clone(),
                        position_ms,
                        duration_ms,
                    });
                }
            }
        }
        self.draw_picture(shared, props, dc, size)
    }

    fn apply(
        applied: &mut Option<VideoProps>,
        decoder: &VideoDecoder,
        props: &VideoProps,
        hold: bool,
    ) -> Result<(), VideoError> {
        let mut effective = props.clone();
        effective.paused |= hold;
        let props = &effective;
        if applied.as_ref().is_none_or(|old| {
            old.paused != props.paused
                || old.active != props.active
                || old.rate != props.rate
                || old.muted != props.muted
                || old.volume != props.volume
        }) {
            decoder.apply(props)?;
            *applied = Some(props.clone());
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.decoder.take();
        self.direct.take();
        self.pending = None;
        self.queued = None;
    }

    fn hold(&self) -> Result<bool, VideoError> {
        match &self.direct {
            Some(direct) => Ok(direct.hold()? || self.queued.is_some()),
            None => Ok(false),
        }
    }
}
