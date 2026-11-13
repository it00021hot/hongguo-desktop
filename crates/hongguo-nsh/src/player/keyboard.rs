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

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Bindings() -> impl IntoElement {
        let shortcuts = Shortcuts::provide_preview();
        let enabled = signal(true);
        let count = signal(0);
        bind(
            &[Action::PlayPause],
            computed(move || enabled.get()),
            move |_| count.update(|count| *count += 1),
        );
        view()
            .flex_1()
            .child(
                crate::ui::button("Change play key", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || shortcuts.capture(Action::PlayPause)),
            )
            .child(
                crate::ui::button("Suspend", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || enabled.set(false)),
            )
            .child(
                crate::ui::button("Resume", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || enabled.set(true)),
            )
            .child(crate::ui::field("Draft"))
            .child(text(move || format!("calls:{}", count.get())))
            .child(text(move || {
                format!("binding:{}", shortcuts.key(Action::PlayPause))
            }))
    }

    #[test]
    fn rebinding_removes_old_keys_and_capture_editing_and_inactive_pages_do_not_play()
    -> Result<(), String> {
        let mut config = EngineConfig::new(Root::of::<Bindings>());
        config.environment.os = nsh_core::platform::Os::MacOs;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("Change play key")?;
        app.press_key("space")?;
        app.wait_for("text:calls:1")?;
        app.press("Change play key")?;
        app.press_key("j")?;
        app.wait_for("text:binding:j")?;
        app.wait_for("text:calls:1")?;
        app.press_key("space")?;
        app.wait_for("text:calls:1")?;
        app.press_key("j")?;
        app.wait_for("text:calls:2")?;
        app.type_text("Draft", "jj")?;
        app.wait_for("text:calls:2")?;
        app.press_key("tab")?;
        app.press("Suspend")?;
        app.press_key("j")?;
        app.wait_for("text:calls:2")?;
        app.press("Resume")?;
        app.press_key("j")?;
        app.wait_for("text:calls:3")?;
        Ok(())
    }
}
