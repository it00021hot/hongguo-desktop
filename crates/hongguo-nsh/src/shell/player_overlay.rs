use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct PlayerOverlay(Signal<Option<State>>);

#[derive(Clone, Copy, PartialEq)]
struct State {
    comments: Signal<bool>,
    fullscreen: Memo<bool>,
}

impl PlayerOverlay {
    pub(crate) fn provide() {
        provide_context(Self(signal(None)));
    }

    pub(crate) fn comments_open() -> bool {
        use_context::<Self>()
            .and_then(|overlay| overlay.0.get())
            .is_some_and(|state| state.comments.get())
    }

    pub(crate) fn fullscreen() -> bool {
        use_context::<Self>()
            .and_then(|overlay| overlay.0.get())
            .is_some_and(|state| state.fullscreen.get())
    }

    pub(crate) fn bind(active: Memo<bool>, comments: Signal<bool>, fullscreen: Memo<bool>) {
        let overlay = use_context::<Self>().expect("player overlay owner");
        let state = State {
            comments,
            fullscreen,
        };
        effect(move || {
            if active.get() {
                overlay.0.set(Some(state));
            } else if overlay.0.get_untracked() == Some(state) {
                overlay.0.set(None);
            }
        });
        cleanup(move || {
            if overlay.0.try_get().flatten() == Some(state) {
                overlay.0.try_set(None);
            }
        });
    }
}
