use super::{Action, Mode, Request, Response, State, WindowModes};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct PreviewDriver(WindowModes);

pub(crate) fn provide_preview() -> PreviewDriver {
    let state = WindowModes {
        state: signal(State::initial()),
        request: signal(None),
        pending: signal(false),
        error: signal(SharedString::default()),
        aspect: signal(super::PHONE_ASPECT),
        applied_aspect: stored(super::PHONE_ASPECT),
        windowed_mode: stored(nsh_core::desktop::WindowMode::Normal),
        preview: true,
    };
    provide_context(state);
    PreviewDriver(state)
}

impl PreviewDriver {
    pub fn pending(self) -> bool {
        self.0.pending()
    }
    pub fn acknowledge(self) {
        if !self.pending() {
            return;
        }
        let Some(Request { serial, action }) = self.0.request.get_untracked() else {
            return;
        };
        let mut state = self.0.state.get_untracked();
        match action {
            Action::SetMode { mode, .. } => {
                state.mode = mode;
                state.portrait_fill = false;
                state.fullscreen = false;
            }
            Action::SetPortraitFill { fill, .. } => state.portrait_fill = fill,
            Action::SetFrameless { enabled } => {
                state.frameless = enabled;
                if enabled {
                    state.fullscreen = false;
                }
            }
            Action::SetIncognito { enabled } => {
                state.incognito = enabled;
                state.visible = true;
            }
            Action::SetLandscape { enabled } => state.landscape = enabled,
            Action::SetPinned { enabled } => state.pinned = enabled,
            Action::ToggleFullscreen => {
                state.frameless = false;
                state.fullscreen = !state.fullscreen;
            }
            Action::Exit => {
                state.mode = Mode::Normal;
                state.portrait_fill = false;
                state.frameless = false;
                state.landscape = false;
                state.fullscreen = false;
            }
            Action::Refit { .. } => {}
        }
        self.0.receive(&nsh_core::element::NativeEvent {
            name: "applied".into(),
            data: serde_json::to_value(Response {
                serial,
                state,
                error: None,
            })
            .expect("preview window response"),
        });
    }
    pub fn fail(self, message: &str) {
        if !self.pending() {
            return;
        }
        let Some(request) = self.0.request.get_untracked() else {
            return;
        };
        self.0.receive(&nsh_core::element::NativeEvent {
            name: "applied".into(),
            data: serde_json::to_value(Response {
                serial: request.serial,
                state: self.0.state.get_untracked(),
                error: Some(message.into()),
            })
            .expect("preview window failure"),
        });
    }
}
