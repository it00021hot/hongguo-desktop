use crate::OutputError;
use hongguo_nsh_video_api::{
    ErrorCode, VideoProps,
    frames::{FrameLease, PlaybackControl as Intent},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub(crate) struct PlaybackControl {
    pub frames: Arc<FrameLease>,
    pub held: Arc<AtomicBool>,
    pub error: Arc<Mutex<Option<OutputError>>>,
    pub intent: Arc<Mutex<Intent>>,
}
impl PlaybackControl {
    pub fn is_playing(&self) -> bool {
        self.frames.is_control_owner().unwrap_or(false)
            && self
                .intent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .playing
            && !self.held.load(Ordering::Acquire)
            && self
                .frames
                .position()
                .ok()
                .flatten()
                .is_some_and(|position| !position.ended)
    }
    pub fn pause(&self) {
        self.held.store(true, Ordering::Release);
        if let Err(error) = self.frames.pause_if_owner() {
            self.publish_error(OutputError::new(ErrorCode::DecoderFailed, error));
        }
    }
    pub fn reconcile(&self, props: &VideoProps) {
        let intent = Intent {
            playing: props.active && !props.paused,
            volume: props.volume,
            muted: props.muted,
            rate: props.rate,
        };
        *self
            .intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = intent;
        if let Err(error) = self.frames.control_if_owner(intent) {
            self.publish_error(OutputError::new(ErrorCode::DecoderFailed, error));
        }
    }
    pub fn apply(&self, props: &VideoProps) {
        let intent = Intent {
            playing: props.active && !props.paused,
            volume: props.volume,
            muted: props.muted,
            rate: props.rate,
        };
        *self
            .intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = intent;
        let result = if props.active {
            self.frames.control(intent)
        } else {
            self.frames.control_if_owner(intent).map(|_| ())
        };
        if let Err(error) = result {
            self.publish_error(OutputError::new(ErrorCode::DecoderFailed, error));
        }
    }
    pub fn publish_error(&self, error: OutputError) {
        *self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
    }
}
