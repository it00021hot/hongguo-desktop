use super::*;

impl Controller {
    pub(super) fn events(
        &mut self,
        shared: &Shared,
        decoder: &VideoDecoder,
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
