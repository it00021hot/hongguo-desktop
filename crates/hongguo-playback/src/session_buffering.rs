use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum VideoWork {
    Waiting,
    Budget,
    SourceRead,
    Decode,
    Queue,
}

fn covers(frame: &PresentationFrame, epoch: u64, position: f64) -> bool {
    frame.generation == epoch
        && frame.pts_secs <= position + DUE_AHEAD_SECS
        && position < frame.pts_secs + frame.duration_secs
}

impl Shared {
    pub(super) fn hold_starved_video(&self, state: &mut State) {
        let position = self.position_locked(state);
        let expected = state
            .index
            .as_ref()
            .is_some_and(|index| index.video.expects_frame_at(position));
        if state.video_stall_started.is_some() && !expected {
            state.holding = false;
            self.release_video_stall(state);
            let playing = state.desired_playing;
            state.mode = state.playback_mode();
            state.silent_clock.set_running(playing, Instant::now());
            if playing {
                let _ = self.output.send(OutputCommand::Start);
            }
        }
        if state.holding || !state.desired_playing {
            return;
        }
        if !expected
            || state
                .current
                .as_ref()
                .is_some_and(|frame| covers(frame, state.epoch, position))
            || state
                .queue
                .iter()
                .any(|next| covers(&next.frame, state.epoch, position))
        {
            return;
        }
        state.holding = true;
        state.video_stall_started = Some(Instant::now());
        state.mode = PlaybackState::Buffering;
        state.silent_clock.set_running(false, Instant::now());
        // Prevent new PCM consumption immediately. Already submitted native audio
        // can still advance played(); its real position remains the master clock.
        self.pcm.suspend();
        let _ = self.output.send(OutputCommand::Pause);
        self.changed.notify_all();
        log::debug!(
            "[playback] video buffering began epoch={} position={position:.3} queued={} decoder_done={} worker_stage={:?} stage_elapsed_ms={}",
            state.epoch,
            state.queue.len(),
            state.video_done,
            state.video_work.0,
            state.video_work.1.elapsed().as_millis()
        );
    }

    pub(super) fn clear_empty_edit(&self, state: &mut State) -> bool {
        let gap = state
            .index
            .as_ref()
            .is_some_and(|index| index.video.is_empty_edit_at(self.position_locked(state)));
        if gap {
            self.hold_starved_video(state);
            state.current = None;
            let position = self.position_locked(state);
            while state.queue.front().is_some_and(|queued| {
                queued.frame.pts_secs + queued.frame.duration_secs <= position
            }) {
                let expired = state.queue.pop_front().expect("front checked");
                state.queue_bytes -= expired.bytes;
            }
            self.changed.notify_all();
            crate::budget::wake();
        }
        gap
    }

    pub(super) fn release_video_stall(&self, state: &mut State) {
        if let Some(started) = state.video_stall_started.take() {
            // Native Pause is asynchronous; retain actual PCM progress, never target_secs.
            self.pcm.activate(state.epoch);
            log::debug!(
                "[playback] video buffering ended epoch={} elapsed_ms={} position={:.3}",
                state.epoch,
                started.elapsed().as_millis(),
                self.position_locked(state)
            );
        }
    }

    pub(super) fn can_resume_video(&self, state: &State) -> bool {
        if state.video_stall_started.is_none() {
            return true;
        }
        let position = self.position_locked(state);
        !state
            .index
            .as_ref()
            .is_some_and(|index| index.video.expects_frame_at(position))
            || state
                .current
                .as_ref()
                .is_some_and(|frame| covers(frame, state.epoch, position))
    }
}
