mod pixel;
#[cfg(test)]
mod tests;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Control {
    reference: ElementRef,
    source: Stored<String>,
    pending: Signal<Option<String>>,
    pixel: Signal<bool>,
}

impl Control {
    pub fn new() -> Self {
        Self {
            reference: element_ref(),
            source: stored(uuid::Uuid::new_v4().to_string()),
            pending: signal(None),
            pixel: signal(false),
        }
    }

    pub fn seek(self, seconds: f64) {
        if !self.pixel.get_untracked() {
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

#[derive(Clone)]
pub(super) struct State {
    pub url: String,
    pub frame_source: Option<String>,
    pub codec_extension: Option<std::rc::Rc<dyn Fn() -> Result<String, String>>>,
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
    let frame_url = signal(state.frame_source.clone());
    let state = stored(state);
    let callbacks = stored(std::rc::Rc::new(callbacks));
    if let Some(enhancement) = crate::player::enhancement::Controller::current() {
        effect(move || {
            let settings = enhancement.settings.get();
            if (!settings.anime4k_on && !settings.sharpen_on) || frame_url.get_untracked().is_some()
            {
                return;
            }
            if let Some(extension) = &state.get().codec_extension {
                match extension() {
                    Ok(url) => frame_url.set(Some(url)),
                    Err(detail) => {
                        log::warn!(
                            "[NSH enhancement] native Video cannot process this source: {detail}"
                        );
                        enhancement
                            .phase
                            .set(hongguo_nsh_video_api::EnhancementPhase::Error);
                        enhancement
                            .message
                            .set(crate::i18n::tr("player.enhanceErrGeneric"));
                    }
                }
            }
        });
    }
    view().flex_1().child(each(
        move || vec![frame_url.get()],
        |url| url.clone(),
        move |url, _| {
            let mut state = state.get();
            state.frame_source = url.get_untracked();
            if pixel_supported() && state.frame_source.is_some() {
                pixel::mount(state, forward(callbacks))
            } else {
                mount_native(state, callbacks, frame_url)
            }
        },
    ))
}

fn forward(callbacks: Stored<std::rc::Rc<Callbacks>>) -> Callbacks {
    Callbacks {
        load: Box::new(move |info| (callbacks.get().load)(info)),
        progress: Box::new(move |info| (callbacks.get().progress)(info)),
        buffer: Box::new(move |buffering| (callbacks.get().buffer)(buffering)),
        error: Box::new(move |error| (callbacks.get().error)(error)),
        end: Box::new(move || (callbacks.get().end)()),
    }
}

fn mount_native(
    state: State,
    callbacks: Stored<std::rc::Rc<Callbacks>>,
    frame_url: Signal<Option<String>>,
) -> View {
    state.control.pixel.set(false);
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
            .on_load(move |info: &VideoInfo| (callbacks.get().load)(info))
            .on_progress(move |info: &VideoProgress| (callbacks.get().progress)(info))
            .on_buffer(move |buffering: &bool| (callbacks.get().buffer)(buffering))
            .on_error(move |error: &str| {
                if frame_url.get_untracked().is_some() {
                    return;
                }
                if let Some(extension) = &state.codec_extension {
                    match extension() {
                        Ok(url) => frame_url.set(Some(url)),
                        Err(detail) => (callbacks.get().error)(&detail),
                    }
                } else {
                    (callbacks.get().error)(error);
                }
            })
            .on_end(move || (callbacks.get().end)()),
    )
}
