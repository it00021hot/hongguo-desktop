use super::{Action, Mode, WindowModes};
use nsh_core::{desktop::TitleBarStyle, prelude::*};

pub(super) fn install(modes: WindowModes) {
    if !matches!(os(), nsh_core::platform::Os::Windows) {
        return;
    }
    effect(move || {
        let state = modes.state.get();
        let mut hidden = state.frameless || state.mode == Mode::Mini;
        if modes.pending.get()
            && let Some(request) = modes.request.get()
        {
            hidden = match request.action {
                Action::SetMode { mode, .. } => state.frameless || mode == Mode::Mini,
                Action::SetFrameless { enabled } => enabled || state.mode == Mode::Mini,
                Action::Exit => false,
                Action::ToggleFullscreen => state.mode == Mode::Mini,
                _ => hidden,
            };
        }
        // Default disables SDK-painted caption buttons; our HWND bridge owns the borderless frame.
        crate::native_window::set_title_bar(if hidden {
            TitleBarStyle::Default
        } else {
            TitleBarStyle::Hidden
        });
    });
}
