mod pixel;
#[cfg(test)]
mod tests;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Control {
    reference: ElementRef,
    source: Stored<String>,
    pending: Signal<Option<String>>,
}

impl Control {
    pub fn new() -> Self {
        Self {
            reference: element_ref(),
            source: stored(uuid::Uuid::new_v4().to_string()),
            pending: signal(None),
        }
    }

    pub fn seek(self, seconds: f64) {
        if !pixel_supported() {
            self.reference.seek(seconds);
            return;
        }
        let mut request = hongguo_nsh_video_api::Seek::default();
        request.source_id = self.source.get();
        request.request_id = uuid::Uuid::new_v4().to_string();
        request.position_ms = (seconds * 1000.0).round() as u64;
        self.pending.set(Some(request.request_id.clone()));
        self.reference.command(
            hongguo_nsh_video_api::SEEK_COMMAND,
            serde_json::to_value(request).expect("typed video seek"),
        );
    }
}

pub(super) const fn pixel_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    ))
}

pub(super) struct State {
    pub url: String,
    pub frame_source: Option<String>,
    pub control: Control,
    pub rate: Memo<f32>,
    pub interval: Memo<f32>,
    pub volume: Signal<f32>,
    pub muted: Memo<bool>,
    pub opacity: Memo<f32>,
    pub paused: Memo<bool>,
    pub active: Memo<bool>,
    pub resize_mode: Memo<hongguo_nsh_video_api::ResizeMode>,
}

pub(super) struct Callbacks {
    pub load: Box<dyn Fn(&VideoInfo)>,
    pub progress: Box<dyn Fn(&VideoProgress)>,
    pub buffer: Box<dyn Fn(&bool)>,
    pub error: Box<dyn Fn(&str)>,
    pub end: Box<dyn Fn()>,
}

pub(super) fn mount(state: State, callbacks: Callbacks) -> View {
    if pixel_supported() {
        return pixel::mount(state, callbacks);
    }
    let callbacks = stored(callbacks);
    view().flex_1().child(
        video(state.url)
            .flex_1()
            .element_ref(&state.control.reference)
            .controls(false)
            .audio_session(|| nsh_core::AudioSession::Playback)
            .object_fit(move || match state.resize_mode.get() {
                hongguo_nsh_video_api::ResizeMode::Contain => nsh_core::ResizeMode::Contain,
                hongguo_nsh_video_api::ResizeMode::Cover => nsh_core::ResizeMode::Cover,
                hongguo_nsh_video_api::ResizeMode::Stretch => nsh_core::ResizeMode::Stretch,
            })
            .rate(state.rate)
            .progress_interval(state.interval)
            .volume(state.volume)
            .muted(state.muted)
            .opacity(state.opacity)
            .paused(state.paused)
            .on_load(move |info: &VideoInfo| callbacks.with(|callbacks| (callbacks.load)(info)))
            .on_progress(move |info: &VideoProgress| {
                callbacks.with(|callbacks| (callbacks.progress)(info))
            })
            .on_buffer(move |buffering: &bool| {
                callbacks.with(|callbacks| (callbacks.buffer)(buffering))
            })
            .on_error(move |error: &str| callbacks.with(|callbacks| (callbacks.error)(error)))
            .on_end(move || callbacks.with(|callbacks| (callbacks.end)())),
    )
}
