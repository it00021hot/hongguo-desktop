use super::*;

impl PlaybackSession {
    pub fn set_playing(&self, playing: bool) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        state.control.playing = playing;
        self.shared.refresh(&mut state)?;
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn set_volume(&self, volume: f32, muted: bool) -> AppResult<()> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(AppError::Media("invalid video volume".into()));
        }
        let mut state = self.shared.state.lock();
        state.check()?;
        state.control.volume = volume;
        state.control.muted = muted;
        self.shared.refresh(&mut state)
    }

    pub fn set_rate(&self, rate: f64) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        state.silent.set_rate(rate, Instant::now())?;
        state.control.rate = rate;
        self.shared.refresh(&mut state)?;
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn seek(&self, position: f64) -> AppResult<u64> {
        if !position.is_finite() || position < 0.0 {
            return Err(AppError::Media("invalid video seek position".into()));
        }
        let mut state = self.shared.state.lock();
        state.check()?;
        let target = state
            .index
            .as_ref()
            .map_or(position, |index| position.min(index.duration_secs));
        self.shared.restart(&mut state, target)?;
        Ok(state.epoch)
    }

    pub fn recover_presentation(&self, detail: String) -> AppResult<()> {
        let mut state = self.shared.state.lock();
        state.check()?;
        if state.recoveries >= self.shared.config.recovery.attempts {
            return Err(AppError::Media(format!(
                "video presentation recovery exhausted: {detail}"
            )));
        }
        self.shared.refresh(&mut state)?;
        state.recoveries += 1;
        let target = state.position;
        self.shared.restart(&mut state, target)
    }
}

impl Shared {
    pub(super) fn restart(&self, state: &mut State, target: f64) -> AppResult<()> {
        let next = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| AppError::Media("video generation exhausted".into()))?;
        if let Some(clock) = &state.clock {
            clock
                .control(Control {
                    playing: false,
                    ..state.control
                })
                .map_err(AppError::Media)?;
            clock.seek(target).map_err(AppError::Media)?;
        }
        state.cancel.store(true, Ordering::Release);
        state.cancel = Arc::new(AtomicBool::new(false));
        state.epoch = next;
        state.target = target;
        state.position = target;
        state.current = None;
        state.queue.clear();
        state.bytes = 0;
        state.holding = true;
        state.presented = false;
        state.video_done = false;
        state.buffering = true;
        state.audio_tail = false;
        state.stalled_at = None;
        state.silent.set_running(false, Instant::now());
        state.silent.seek(target, Instant::now())?;
        self.changed.notify_all();
        crate::budget::wake();
        Ok(())
    }
}
