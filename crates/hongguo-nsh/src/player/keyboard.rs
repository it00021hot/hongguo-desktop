use crate::shortcuts::{Action, Shortcuts};
use nsh_core::{keymap::When, prelude::*};

pub(crate) fn bind(
    actions: &'static [Action],
    enabled: Memo<bool>,
    dispatch: impl Fn(Action) + Copy + 'static,
) {
    let Some(shortcuts) = Shortcuts::current() else {
        return;
    };
    effect(move || {
        if !enabled.get() || shortcuts.is_capturing() {
            return;
        }
        for &action in actions {
            on_key_with(
                &shortcuts.key(action),
                KeyOptions {
                    when: When::NotEditing,
                    repeat: false,
                    ..Default::default()
                },
                move |_| {
                    if enabled.get_untracked() && !shortcuts.is_capturing() {
                        dispatch(action);
                    }
                },
            );
        }
    });
}
