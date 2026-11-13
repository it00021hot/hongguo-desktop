use hongguo_nsh_video_api::VideoProps;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};

#[derive(Clone)]
pub(crate) struct PlaybackControl(pub Retained<AnyObject>);

// SAFETY: AVPlayer's playback controls are thread-safe; this handle never touches AppKit/UIKit views.
unsafe impl Send for PlaybackControl {}
// SAFETY: Shared access is limited to AVPlayer's documented thread-safe playback setters.
unsafe impl Sync for PlaybackControl {}

impl PlaybackControl {
    pub fn is_playing(&self) -> bool {
        // SAFETY: Read-only public AVPlayer state. Waiting is still an active play
        // request, unlike a real end/pause, so it must keep its playback audio session.
        unsafe {
            let rate: f32 = msg_send![&self.0, rate];
            let status: i64 = msg_send![&self.0, timeControlStatus];
            rate > 0.0 || status == 1
        }
    }
    pub fn pause(&self) {
        // SAFETY: Public control on a live retained AVPlayer.
        unsafe {
            let _: () = msg_send![&self.0, pause];
        }
    }

    pub fn apply(&self, props: &VideoProps) {
        // SAFETY: Props were validated before publication and AVPlayer accepts these setters across threads.
        unsafe {
            let _: () = msg_send![&self.0, setVolume: props.volume];
            let _: () = msg_send![&self.0, setMuted: Bool::new(props.muted)];
            if props.paused || !props.active {
                let _: () = msg_send![&self.0, pause];
            } else {
                let _: () = msg_send![&self.0, playImmediatelyAtRate: props.rate as f32];
            }
        }
    }
}
