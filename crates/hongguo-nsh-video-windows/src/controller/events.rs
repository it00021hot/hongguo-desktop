use super::*;

impl Controller {
    pub(super) fn events(
        &mut self,
        shared: &Shared,
        decoder: &VideoDecoder,
        props: &VideoProps,
        dc: &ID2D1DeviceContext,
    ) -> Result<(), VideoError> {
        let source_id = decoder.source().id.clone();
        while let Some(event) = decoder.next_event()? {
            match event {
                DecoderEvent::Ready if self.direct.is_some() => {
                    self.direct
                        .as_mut()
                        .expect("direct frame source")
                        .audio_ready = true;
                }
                DecoderEvent::Metadata | DecoderEvent::Ready
                    if !self.loaded && self.direct.is_none() =>
                {
                    let (width, height) = decoder.dimensions()?;
                    if width > 0 && height > 0 {
                        self.raw = Some(RawFrame::new(dc, width, height)?);
                        self.loaded = true;
                        shared.emit(VideoEvent::Load {
                            source_id: source_id.clone(),
                            duration_ms: decoder.duration_ms(),
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
                    if let Some(direct) = &mut self.direct {
                        if self.pending.is_none() || decoder.seeking() {
                            continue;
                        }
                        direct.audio_seeked();
                        if let Some(next) = self.queued.take() {
                            decoder.pause()?;
                            direct.seek(next.position_ms)?;
                            decoder.seek(next.position_ms)?;
                            self.pending = Some((next, Instant::now()));
                        }
                        continue;
                    }
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
                            // A repeat may have reached EOF and paused without a props change.
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
        Ok(())
    }
}
