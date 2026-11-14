use hongguo_native_clock::{Clock, Control};
use hongguo_nsh_video_api::frames::{
    FrameInfo, FrameSource, PixelFrame, PlaybackControl, PlaybackPosition,
};
use std::sync::{Arc, Mutex};

/// The fixture uses the same platform audio factory as production playback.
pub(super) struct Clocked {
    frames: Arc<dyn FrameSource>,
    audio_url: String,
    state: Mutex<(Control, Option<Arc<dyn Clock>>)>,
}

impl Clocked {
    pub(super) fn new(frames: Arc<dyn FrameSource>, audio_url: String) -> Self {
        Self {
            frames,
            audio_url,
            state: Mutex::new((Control::default(), None)),
        }
    }
    fn clock(&self) -> Option<Arc<dyn Clock>> {
        self.state.lock().expect("probe clock state").1.clone()
    }
}

impl FrameSource for Clocked {
    fn configure_native_clock(
        &self,
        factory: &dyn Fn(String) -> Result<Arc<dyn Clock>, String>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().expect("probe clock state");
        if state.1.is_none() {
            let clock = factory(self.audio_url.clone())?;
            clock.control(state.0)?;
            state.1 = Some(clock);
        }
        Ok(())
    }
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        if let Some(clock) = self.clock()
            && clock.position()?.ready
        {
            self.frames.info()
        } else {
            Ok(None)
        }
    }
    fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        self.frames.frame_at(position_ms)
    }
    fn seek(&self, position_ms: u64) -> Result<u64, String> {
        let generation = self.frames.seek(position_ms)?;
        log::info!("PIXEL_SEEK positionMs={position_ms} generation={generation}");
        if let Some(clock) = self.clock() {
            clock.seek(position_ms as f64 / 1000.0)?;
        }
        Ok(generation)
    }
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        log::info!("PIXEL_PRESENTED generation={generation} serial={serial}");
        self.frames.presented(generation, serial)
    }
    fn holding(&self) -> bool {
        self.frames.holding()
    }
    fn control(&self, control: PlaybackControl) -> Result<(), String> {
        let mut state = self.state.lock().expect("probe clock state");
        state.0 = Control {
            playing: control.playing,
            rate: control.rate,
            volume: control.volume,
            muted: control.muted,
        };
        if let Some(clock) = &state.1 {
            clock.control(state.0)?;
        }
        Ok(())
    }
    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        self.clock()
            .map(|clock| {
                clock.position().map(|position| PlaybackPosition {
                    position_secs: position.seconds,
                    duration_secs: 2.0,
                    ended: position.ended,
                    buffering: position.buffering || !position.ready,
                    video_gap: false,
                })
            })
            .transpose()
    }
    fn close(&self) {
        if let Some(clock) = self.clock() {
            clock.close();
        }
        self.frames.close();
    }
}
