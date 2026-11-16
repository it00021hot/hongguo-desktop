use super::*;

impl Shared {
    pub(super) fn refresh(&self, state: &mut State) -> AppResult<()> {
        state.check()?;
        let now = Instant::now();
        let Some(index) = &state.index else {
            return Ok(());
        };
        let audio = if index.audio.is_some() {
            state
                .clock
                .as_ref()
                .map(|clock| clock.position().map_err(AppError::Media))
                .transpose()?
        } else {
            None
        };
        let audio_ready =
            index.audio.is_none() || audio.is_some_and(|audio| audio.ready && !audio.seeking);
        let audio_buffering = audio.is_some_and(|audio| audio.buffering);
        let position = if index.audio.is_none() || state.audio_tail {
            state.silent.position(now)
        } else if let Some(audio) = audio.filter(|audio| audio.ready && !audio.seeking) {
            if audio.ended && audio.seconds < index.duration_secs {
                state.audio_tail = true;
                state.silent.seek(audio.seconds, now)?;
            }
            audio.seconds
        } else {
            state.position
        };
        state.position = position.min(index.duration_secs);
        let expected = index.video.expects_frame_at(state.position);
        state.video_gap = !expected;
        let missing = expected
            && state.current.as_ref().is_none_or(|frame| {
                frame.pts_secs + frame.duration_secs + DUE_AHEAD_SECS < state.position
            })
            && state.queue.is_empty();
        if missing && state.video_done {
            return Err(AppError::Media(
                "video decoder ended before the expected frame".into(),
            ));
        }
        if missing && state.presented {
            state.rebuffering = true;
            state.current = None;
        }
        if missing && state.control.playing {
            state.stalled_at.get_or_insert(now);
        } else {
            state.stalled_at = None;
        }
        if state.control.playing
            && state
                .stalled_at
                .is_some_and(|start| now.duration_since(start) > self.config.recovery.elapsed)
        {
            return Err(AppError::Media(
                "video decoder stalled beyond recovery deadline".into(),
            ));
        }
        if state.rebuffering {
            let ready_until = state
                .queue
                .back()
                .or(state.current.as_ref())
                .map_or(state.position, |frame| frame.pts_secs + frame.duration_secs);
            let recovered = ready_until - state.position
                >= REBUFFER_SECS.min(self.config.video_ahead_secs) * state.control.rate;
            if recovered || state.queue_full || state.video_done || !expected {
                state.rebuffering = false;
            }
        }
        let holding =
            !audio_ready || (expected && (!state.presented || missing || state.rebuffering));
        let buffering = holding || audio_buffering;
        let playing = state.control.playing && !buffering && state.position < index.duration_secs;
        state.silent.set_running(playing, now);
        if let Some(clock) = &state.clock {
            clock
                .control(Control {
                    playing: playing && !state.audio_tail,
                    ..state.control
                })
                .map_err(AppError::Media)?;
        }
        state.holding = holding;
        state.buffering = buffering;
        Ok(())
    }
}
