use super::{ButtonVariant, button, field};
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Controls() -> impl IntoElement {
    let value = signal(String::new());
    let count = signal(0_u32);
    view()
        .flex_1()
        .child(
            field("Search")
                .test_id("query")
                .value(value)
                .on_change(move |next: &str| value.set(next.to_owned())),
        )
        .child(
            button("Add", ButtonVariant::Primary)
                .on_press(move || count.update(|count| *count += 1)),
        )
        .child(
            button("Unavailable", ButtonVariant::Secondary)
                .disabled(true)
                .on_press(move || count.set(999)),
        )
        .child(text(move || format!("{}:{}", count.get(), value.get())))
}

#[test]
fn styled_controls_keep_their_behavior_and_names() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Controls>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("Add")?;
    app.type_text("id:query", "drama")?;
    app.press("role:button[Add]")?;
    app.wait_for("1:drama")?;
    assert!(!app.screen().enabled("Unavailable")?);
    Ok(())
}

#[component]
fn Routes() -> impl IntoElement {
    set_tokens(
        nsh_base::tokens_of(Some(include_str!("../../theme.json"))).expect("validated NSH theme"),
    );
    let nav = crate::navigation::Navigation::new();
    super::stack().child(each(
        nav.routes(),
        |route: &crate::navigation::Route| format!("{route:?}"),
        move |route, index| {
            let detail = matches!(route.get_untracked(), crate::navigation::Route::Detail(_));
            let title = if detail { "Detail" } else { "Catalog" };
            super::page(nav, title)
                .child(
                    button("Open detail", ButtonVariant::Primary)
                        .on_press(move || nav.push(crate::navigation::Route::Detail("123".into()))),
                )
                .on_dismiss(move || nav.dismiss(index.get_untracked()))
        },
    ))
}

#[test]
fn system_back_updates_navigation_state() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Routes>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("Open detail")?;
    app.press("Open detail")?;
    app.wait_for("Detail")?;
    app.back()?;
    app.press("Open detail")?;
    app.wait_for("Detail")?;
    Ok(())
}

#[component]
fn Sections() -> impl IntoElement {
    set_tokens(
        nsh_base::tokens_of(Some(include_str!("../../theme.json"))).expect("validated NSH theme"),
    );
    let nav = crate::navigation::Navigation::new();
    view()
        .flex_1()
        .child(
            button("Open history playback", ButtonVariant::Primary).on_press(move || {
                nav.push(crate::navigation::Route::History);
                nav.push(crate::navigation::Route::Player("123".into(), 1));
            }),
        )
        .child(text(move || {
            format!("depth:{}", nav.routes().with(Vec::len))
        }))
        .child(super::sections(nav))
}

#[test]
fn playback_highlights_home_and_section_navigation_returns_to_the_home_root() -> Result<(), String>
{
    let mut config = EngineConfig::new(Root::of::<Sections>().tokens(
        nsh_base::tokens_of(Some(include_str!("../../theme.json"))).expect("validated NSH theme"),
    ));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Open history playback")?;
    assert!(app.screen().selected("role:button[Discover]")?);
    app.press_key("tab")?;
    app.press_key("enter")?;
    app.wait_for("text:depth:3")?;
    assert!(!app.screen().selected("role:button[History]")?);
    assert!(app.screen().selected("role:button[Discover]")?);
    app.press_key("tab")?;
    app.press_key("enter")?;
    app.wait_for("text:depth:1")?;
    assert!(app.screen().selected("role:button[Discover]")?);
    Ok(())
}

#[component]
fn OverflowingForm() -> impl IntoElement {
    let saved = signal(false);
    super::form(
        view()
            .gap_4()
            .children((0..20).map(|index| text(format!("Setting {index}"))))
            .child(button("Save form", ButtonVariant::Primary).on_press(move || saved.set(true)))
            .child(text(
                move || if saved.get() { "Form saved" } else { "Unsaved" },
            )),
    )
    .test_id("form")
}

#[test]
fn forms_remain_scrollable_with_the_software_keyboard() -> Result<(), String> {
    let mut config = EngineConfig::new(Root::of::<OverflowingForm>());
    config.viewport = nsh_engine::Size::new(px(390.0), px(400.0));
    config.environment.keyboard_height = px(240.0);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Setting 0")?;
    app.scroll("id:form", px(1500.0))?;
    app.press("Save form")?;
    app.wait_for("Form saved")?;
    Ok(())
}

#[component]
fn SettingsSwitches() -> impl IntoElement {
    let enabled = signal(false);
    view()
        .child(
            super::switch("Autoplay")
                .checked(enabled)
                .on_change(move |next: &bool| enabled.set(*next)),
        )
        .child(
            super::switch("Unavailable setting")
                .checked(false)
                .disabled(true)
                .on_change(move |_: &bool| enabled.set(false)),
        )
        .child(text(move || {
            if enabled.get() {
                "Autoplay enabled"
            } else {
                "Autoplay disabled"
            }
        }))
}

#[test]
fn settings_switch_has_native_semantics_keyboard_and_disabled_behavior() -> Result<(), String> {
    let mut config = EngineConfig::new(Root::of::<SettingsSwitches>());
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Autoplay disabled")?;
    app.press("role:switch[Autoplay]")?;
    app.wait_for("Autoplay enabled")?;
    assert!(!app.screen().enabled("role:switch[Unavailable setting]")?);
    app.press_key("tab")?;
    app.press_key("space")?;
    app.wait_for("Autoplay disabled")?;
    Ok(())
}
