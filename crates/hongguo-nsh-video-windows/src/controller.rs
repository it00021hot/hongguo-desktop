use crate::{
    DecoderEvent, DisplayedFrame, RawFrame, VideoDecoder, VideoError, drawing_device,
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
            if self.loaded {
                Self::apply(&mut self.applied, decoder, props)?;
            } else if props.paused || !props.active {
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
        if self.pending.is_some() || !self.loaded {
            self.queued = Some(request);
            return Ok(());
        }
        if let Some(decoder) = &self.decoder {
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
            let wake = shared.clone();
            self.decoder = Some(VideoDecoder::open(&device, source.clone(), move || {
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
        while let Some(event) = decoder.next_event()? {
            match event {
                DecoderEvent::Metadata | DecoderEvent::Ready if !self.loaded => {
                    let (width, height) = decoder.dimensions()?;
                    if width > 0 && height > 0 {
                        let duration_ms = decoder.duration_ms();
                        self.raw = Some(RawFrame::new(dc, width, height)?);
                        self.loaded = true;
                        shared.emit(VideoEvent::Load {
                            source_id: source_id.clone(),
                            duration_ms,
                            width,
                            height,
                        });
                    }
                }
                DecoderEvent::Buffering(is_buffering) => shared.emit(VideoEvent::Buffer {
                    source_id: source_id.clone(),
                    is_buffering,
                }),
                DecoderEvent::Ended if self.pending.is_none() => shared.emit(VideoEvent::End {
                    source_id: source_id.clone(),
                }),
                DecoderEvent::Seeked => {
                    if let Some((completed, _)) = self.pending.take() {
                        if let Some(next) = self.queued.take() {
                            decoder.seek(next.position_ms)?;
                            self.pending = Some((next, Instant::now()));
                        } else if let Some(position_ms) = decoder.position_ms() {
                            shared.emit(VideoEvent::Seeked {
                                source_id: source_id.clone(),
                                request_id: completed.request_id,
                                position_ms,
                            });
                            // MediaEngine may have paused itself at EOF even though
                            // the caller's playing props did not change for a repeat.
                            decoder.apply(props)?;
                            self.applied = Some(props.clone());
                        } else {
                            return Err(VideoError::new(
                                ErrorCode::SeekFailed,
                                "native seek completed without a finite position",
                            ));
                        }
                    }
                }
                DecoderEvent::Failed { kind, code } => {
                    return Err(VideoError::new(
                        ErrorCode::DecoderFailed,
                        format!("MediaEngine failed: kind={kind}, HRESULT=0x{code:08x}"),
                    ));
                }
                _ => {}
            }
        }
        if self.loaded {
            Self::apply(&mut self.applied, decoder, props)?;
            if self.pending.is_none()
                && let Some(request) = self.queued.take()
            {
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
            if self.raw.is_none() {
                let (width, height) = decoder.dimensions()?;
                self.raw = Some(RawFrame::new(dc, width, height)?);
            }
            if props.active && self.pending.is_none() {
                let mut snapshot = shared
                    .raw
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let raw = self.raw.as_mut().expect("raw frame initialized");
                if raw.capture(decoder.engine())? {
                    *snapshot = raw.metadata().map(|metadata| RawSnapshot {
                        device: self
                            .device
                            .as_ref()
                            .expect("decoder drawing device")
                            .clone(),
                        texture: raw.texture().clone(),
                        metadata,
                    });
                }
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
        if !props.active {
            self.bitmap = None;
            return Ok(());
        }
        let picture = shared
            .picture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|picture| shared.valid(picture.revision))
            .map(|picture| picture.frame.clone());
        if let Some(frame) = picture {
            let result = (|| {
                if self
                    .bitmap
                    .as_ref()
                    .is_none_or(|(old, _)| !old.same_slot(&frame))
                {
                    self.bitmap = Some((frame.clone(), frame.bitmap(dc)?));
                }
                frame.draw(
                    dc,
                    &self.bitmap.as_ref().expect("bitmap initialized").1,
                    size,
                    props.resize_mode,
                )
            })();
            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.bitmap = None;
                    *shared
                        .gpu_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
                }
            }
        } else {
            self.bitmap = None;
        }
        if let Some(raw) = &self.raw {
            raw.draw(dc, size, props.resize_mode)?;
        }
        Ok(())
    }

    fn apply(
        applied: &mut Option<VideoProps>,
        decoder: &VideoDecoder,
        props: &VideoProps,
    ) -> Result<(), VideoError> {
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
        self.pending = None;
        self.queued = None;
    }
}
