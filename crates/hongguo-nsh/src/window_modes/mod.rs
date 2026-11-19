#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod framework;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod geometry;
#[cfg(target_os = "macos")]
mod occlusion;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod placement;
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod privacy;
mod types;
mod wire;
use nsh_core::storage::Storage;
use nsh_core::{desktop::WindowMode, prelude::*};

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use placement::show_main_window;
pub(crate) use types::{Mode, portrait_aspect};
use types::{PHONE_ASPECT, REFIT_DELTA};
pub(crate) use wire::State;
use wire::{Action, Request, Response};

const INCOGNITO_KEY: &str = "guava.incognito";

#[derive(Clone, Copy)]
pub(crate) struct WindowModes {
    exposed: Signal<bool>,
    state: Signal<State>,
    request: Signal<Option<Request>>,
    pending: Signal<bool>,
    error: Signal<SharedString>,
    aspect: Signal<f64>,
    applied_aspect: Stored<f64>,
    windowed_mode: Stored<WindowMode>,
    mini_controls_visible: Signal<bool>,
    native_controls_hidden: Signal<bool>,
    observation: Signal<wire::Observation>,
}

impl WindowModes {
    pub fn set_mini_controls_visible(self, visible: bool) {
        self.mini_controls_visible.set(visible);
    }
    pub fn exposed(self) -> bool {
        self.exposed.get()
    }
    pub fn current() -> Option<Self> {
        use_context()
    }
    pub fn state(self) -> State {
        self.state.get()
    }
    pub fn can_play(self) -> bool {
        self.state.get().visible
    }
    pub fn pending(self) -> bool {
        self.pending.get()
    }
    pub fn error(self) -> SharedString {
        self.error.get()
    }
    pub fn immersive(self) -> bool {
        let s = self.state.get();
        s.mode != Mode::Normal || s.frameless || s.landscape || s.fullscreen
    }
    pub fn toggle_mini(self) {
        let mode = if self.state.get_untracked().mode == Mode::Mini {
            Mode::Normal
        } else {
            Mode::Mini
        };
        self.set_mode(mode, PHONE_ASPECT);
    }
    pub fn toggle_portrait(self, aspect: f64) {
        let mode = if self.state.get_untracked().mode == Mode::Portrait {
            Mode::Normal
        } else {
            Mode::Portrait
        };
        self.set_mode(mode, aspect);
    }
    pub fn set_mode(self, mode: Mode, aspect: f64) {
        self.aspect.set(aspect);
        self.send(Action::SetMode { mode, aspect });
    }
    pub fn toggle_portrait_fill(self, aspect: f64) {
        self.aspect.set(aspect);
        self.send(Action::SetPortraitFill {
            fill: !self.state.get_untracked().portrait_fill,
            aspect,
        });
    }
    pub fn refit(self, aspect: f64) {
        self.aspect.set(aspect);
        if self.state.get_untracked().mode == Mode::Portrait && !self.pending.get_untracked() {
            self.send(Action::Refit { aspect });
        }
    }
    pub fn toggle_frameless(self) {
        self.send(Action::SetFrameless {
            enabled: !self.state.get_untracked().frameless,
        });
    }
    pub fn toggle_incognito(self) {
        self.send(Action::SetIncognito {
            enabled: !self.state.get_untracked().incognito,
        });
    }
    pub fn set_landscape(self, enabled: bool) {
        self.send(Action::SetLandscape { enabled });
    }
    pub fn toggle_pinned(self) {
        self.send(Action::SetPinned {
            enabled: !self.state.get_untracked().pinned,
        });
    }
    pub fn exit(self) {
        self.send(Action::Exit);
    }
    pub fn toggle_fullscreen(self) {
        if !is_desktop() {
            self.set_landscape(!self.state.get_untracked().landscape);
            return;
        }
        if self.state.get_untracked().mode == Mode::Portrait {
            self.toggle_portrait_fill(self.aspect.get_untracked());
            return;
        }
        self.send(Action::ToggleFullscreen);
    }
    fn send(self, action: Action) {
        if self.pending.get_untracked() {
            return;
        }
        if !is_desktop() && !matches!(action, Action::SetLandscape { .. } | Action::Exit) {
            self.error.set(crate::i18n::tr("nsh.windowModeUnavailable"));
            return;
        }
        let serial = self.request.with_untracked(|request| {
            request
                .as_ref()
                .map_or(1, |request| request.serial.wrapping_add(1))
        });
        self.error.set(SharedString::default());
        self.pending.set(true);
        self.request.set(Some(Request { serial, action }));
    }
    fn receive(self, event: &nsh_core::element::NativeEvent) {
        match event.name.as_ref() {
            "observation" => {
                match serde_json::from_value::<wire::Observation>(event.data.clone()) {
                    Ok(observation) => {
                        if let Some(exposed) = observation.exposed {
                            self.exposed.set(exposed);
                        }
                        self.observation.set(observation);
                    }
                    Err(error) => self.fail(format!("invalid window observation: {error}")),
                }
            }
            "orientation" => {
                match serde_json::from_value::<wire::MobileResponse>(event.data.clone()) {
                    Ok(response) => {
                        let mut state = self.state.get_untracked();
                        state.landscape = response.landscape;
                        self.applied(Response {
                            serial: response.serial,
                            state,
                            error: response.error,
                        });
                    }
                    Err(error) => self.fail(format!("invalid orientation response: {error}")),
                }
            }
            "failure" => self.fail(
                event
                    .data
                    .as_str()
                    .unwrap_or("invalid native window failure")
                    .to_owned(),
            ),
            _ => {}
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn desktop_event(self, event: desktop::Event) {
        match event {
            desktop::Event::Applied(response) => self.applied(response),
            desktop::Event::State(state) => self.accept(state),
            desktop::Event::Failure(error) => self.fail(error),
            desktop::Event::Fullscreen { toggle } => {
                let window = nsh_core::element::desktop::current_window();
                if let Some(state) = window.state() {
                    if state.mode == WindowMode::FullScreen {
                        window.set_mode(self.windowed_mode.get());
                    } else if toggle {
                        self.windowed_mode.replace(state.mode);
                        window.set_mode(WindowMode::FullScreen);
                    }
                }
            }
        }
    }

    fn applied(self, response: Response) {
        if self
            .request
            .with_untracked(|request| request.as_ref().map(|request| request.serial))
            != Some(response.serial)
        {
            return;
        }
        let success = response.error.is_none();
        self.pending.set(false);
        if let Some(error) = response.error {
            self.error.set(error.into());
        } else if let Some(aspect) = self.request.with_untracked(|request| {
            request.as_ref().and_then(|request| match request.action {
                Action::SetMode { aspect, .. }
                | Action::SetPortraitFill { aspect, .. }
                | Action::Refit { aspect } => Some(aspect),
                _ => None,
            })
        }) {
            self.applied_aspect.replace(aspect);
        }
        self.accept(response.state);
        let aspect = self.aspect.get_untracked();
        if success
            && response.state.mode == Mode::Portrait
            && (aspect - self.applied_aspect.get()).abs() > REFIT_DELTA
        {
            self.send(Action::Refit { aspect });
        }
    }

    fn fail(self, error: String) {
        log::error!("[NSH window] {error}");
        self.pending.set(false);
        self.error.set(error.into());
    }

    fn accept(self, state: State) {
        let previous = self.state.get_untracked();
        self.state.set(state);
        if previous.incognito != state.incognito
            && let Err(error) = Storage::set_item(INCOGNITO_KEY, state.incognito.to_string())
        {
            self.error.set(error.to_string().into());
        }
    }
}

pub(crate) fn install(observe_visibility: Memo<bool>) -> impl IntoElement {
    let state = WindowModes {
        exposed: signal(true),
        state: signal(State::initial()),
        request: signal(None),
        pending: signal(false),
        error: signal(SharedString::default()),
        aspect: signal(PHONE_ASPECT),
        applied_aspect: stored(PHONE_ASPECT),
        windowed_mode: stored(WindowMode::Normal),
        mini_controls_visible: signal(true),
        native_controls_hidden: signal(false),
        observation: signal(wire::Observation::default()),
    };
    provide_context(state);
    let window = nsh_core::element::desktop::current_window();
    effect(move || {
        if let Some(window) = window.state()
            && window.mode != WindowMode::FullScreen
        {
            state.windowed_mode.replace(window.mode);
        }
    });
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    framework::install(state);
    let initial = Storage::get_item(INCOGNITO_KEY).as_deref() == Some("true");
    if initial && is_desktop() {
        spawn(async move {
            next_frame().await;
            state.send(Action::SetIncognito { enabled: true });
        });
    }
    view()
        .absolute()
        .size(px(1.0))
        .opacity(0.0)
        .pointer_events(PointerEvents::None)
        .child(
            native("hongguo.window-modes")
                .size(px(1.0))
                .prop("controlsHidden", move || {
                    serde_json::Value::Bool(state.native_controls_hidden.get())
                })
                .prop("observeVisibility", move || {
                    serde_json::Value::Bool(observe_visibility.get())
                })
                .prop("request", move || {
                    let request = state.request.get().filter(|_| !is_desktop());
                    serde_json::to_value(request.map(|request| wire::MobileRequest {
                        serial: request.serial,
                        landscape: matches!(request.action, Action::SetLandscape { enabled: true }),
                    }))
                    .expect("mobile window request")
                })
                .on_native_event(move |event: &nsh_core::element::NativeEvent| {
                    state.receive(event)
                }),
        )
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
pub(crate) use platform::register;
