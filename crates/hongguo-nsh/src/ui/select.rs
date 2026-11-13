use nsh_core::prelude::*;
mod presenter;

pub(crate) use nsh_base::SelectItem as SelectOption;
pub(crate) use presenter::ChoiceSelectElement as SelectControl;

pub(crate) fn select(
    label: impl IntoReactive<SharedString>,
    options: impl IntoReactive<Vec<SelectOption>>,
    value: impl IntoReactive<Option<SharedString>>,
) -> SelectControl {
    let label = label.into_reactive();
    let options = options.into_reactive();
    let value = value.into_reactive();
    presenter::choice_select()
        .label(move || label.get())
        .items(move || options.get())
        .value(move || value.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Choices() -> impl IntoElement {
        let value = signal(Some(SharedString::from("first")));
        view().child(
            select(
                "Naming",
                || {
                    vec![
                        SelectOption::new("first", "Series episode"),
                        SelectOption::new("disabled", "Unavailable").disabled(true),
                        SelectOption::new("last", "Series only"),
                    ]
                },
                value,
            )
            .on_change(move |next: &Option<SharedString>| value.set(next.clone())),
        )
    }

    #[component]
    fn LongChoices() -> impl IntoElement {
        let value = signal(None::<SharedString>);
        view().child(
            select(
                "Library",
                || {
                    (0..1000)
                        .map(|at| SelectOption::new(at.to_string(), format!("Choice {at}")))
                        .collect::<Vec<_>>()
                },
                value,
            )
            .on_change(move |next: &Option<SharedString>| value.set(next.clone())),
        )
    }

    #[test]
    fn long_select_virtualizes_and_keyboard_end_targets_the_recycled_final_option()
    -> Result<(), String> {
        let mut config = EngineConfig::new(Root::of::<LongChoices>());
        config.environment.os = nsh_core::platform::Os::MacOs;
        config.environment.reduce_motion = true;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("role:combobox[Library]")?;
        app.press("role:combobox[Library]")?;
        app.wait_for("role:option[Choice 0]")?;
        assert!(
            app.screen().count("role:option")? < 80,
            "candidate rows must be virtualized"
        );
        app.press_key("end")?;
        app.wait_for("role:option[Choice 999]")?;
        app.press_key("enter")?;
        app.wait_for("role:combobox[Library] >> text:Choice 999")?;
        app.press_key("enter")?;
        app.wait_for("role:option[Choice 999]")?;
        app.press_key("home")?;
        app.wait_for("role:option[Choice 0]")?;
        app.press_key("enter")?;
        app.wait_for("role:combobox[Library] >> text:Choice 0")?;
        Ok(())
    }

    #[test]
    fn select_moves_real_keyboard_focus_to_the_chosen_option_before_confirmation()
    -> Result<(), String> {
        let mut config = EngineConfig::new(Root::of::<Choices>());
        config.environment.os = nsh_core::platform::Os::MacOs;
        config.environment.reduce_motion = true;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("role:combobox[Naming]")?;
        app.press("role:combobox[Naming]")?;
        app.wait_for("role:option[Series only]")?;
        app.press_key("end")?;
        let option = app
            .screen()
            .press_target("role:option[Series only]")?
            .unwrap();
        assert!(
            app.screen()
                .commands()
                .iter()
                .any(|(id, command)| *id == option && matches!(command, nsh_core::Command::Focus)),
            "native Enter must address the option, not the combobox trigger"
        );
        app.press_key("enter")?;
        app.wait_for("role:combobox[Naming] >> text:Series only")?;
        app.press_key("enter")?;
        app.wait_for("role:option[Series episode]")?;
        app.press_key("home")?;
        app.press_key("down")?;
        let last = app
            .screen()
            .press_target("role:option[Series only]")?
            .unwrap();
        assert!(
            app.screen()
                .commands()
                .iter()
                .rev()
                .find(|(_, command)| matches!(command, nsh_core::Command::Focus))
                .is_some_and(|(id, _)| *id == last),
            "arrow navigation must skip the disabled option"
        );
        app.press_key("escape")?;
        app.wait_for("role:combobox[Naming] >> text:Series only")?;
        Ok(())
    }

    #[test]
    fn mobile_select_uses_the_same_controlled_value_and_closes_after_selection()
    -> Result<(), String> {
        let mut config = EngineConfig::new(Root::of::<Choices>());
        config.environment.os = nsh_core::platform::Os::Ios;
        config.environment.reduce_motion = true;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("role:combobox[Naming]")?;
        app.press("role:combobox[Naming]")?;
        app.wait_for("role:option[Series only]")?;
        assert!(!app.screen().enabled("role:option[Unavailable]")?);
        app.press("role:option[Series only]")?;
        app.wait_for_gone(
            "role:option[Series only]",
            std::time::Duration::from_secs(1),
        )?;
        app.wait_for("role:combobox[Naming] >> text:Series only")?;
        app.press("role:combobox[Naming]")?;
        assert!(app.screen().selected("role:option[Series only]")?);
        app.back()?;
        app.wait_for_gone(
            "role:option[Series only]",
            std::time::Duration::from_secs(1),
        )?;
        Ok(())
    }
}
