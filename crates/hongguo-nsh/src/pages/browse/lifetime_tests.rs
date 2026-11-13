use crate::{
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root, Size};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[component]
fn SearchLifetime() -> impl IntoElement {
    ui::test_log::watch("retained-search-toolbar");
    let nav = Navigation::new();
    let revision = signal(0_u32);
    let pages = ui::stack().child(each(
        nav.routes(),
        |route: &Route| format!("{route:?}"),
        move |route, _| match route.get_untracked() {
            Route::Browse => {
                let label = computed(move || {
                    SharedString::from(format!("Browse controls {}", revision.get()))
                });
                ui::toolbar_slot(
                    move || {
                        nav.routes()
                            .with(|routes| routes.last() == Some(&Route::Browse))
                    },
                    move || text(label),
                );
                super::preview(nav).into_element()
            }
            Route::Detail(_) => ui::page(nav, "Fixture detail")
                .child(text("Detail is healthy"))
                .into_element(),
            _ => ui::page(nav, "Fixture home")
                .child(text("Home is healthy"))
                .into_element(),
        },
    ));
    ui::shell(nav, text("Default controls"), pages).child(
        ui::button("Refresh shell", ui::ButtonVariant::Quiet)
            .on_press(move || revision.update(|revision| *revision += 1)),
    )
}

#[test]
fn retained_search_and_memo_toolbar_survive_detail_and_are_disposed_when_the_section_is_removed()
-> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<SearchLifetime>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.viewport = Size::new(px(1280.0), px(840.0));
    config.clock = Clock::manual(SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Default controls")?;
    for round in 0..3 {
        app.press("role:button[Find]")?;
        app.wait_for(&format!("Browse controls {round}"))?;
        app.type_text("id:browse-query", "装穷")?;
        app.run_for(Duration::from_millis(300))?;
        app.wait_for("id:browse-suggestions")?;
        app.press_key("enter")?;
        app.wait_for("Detail is healthy")?;
        app.wait_for("Default controls")?;
        app.press("role:button[Back]")?;
        app.wait_for(&format!("Browse controls {round}"))?;
        app.press("role:button[Discover]")?;
        app.wait_for("Home is healthy")?;
        app.press("role:button[Refresh shell]")?;
        app.run_for(Duration::from_millis(300))?;
        app.wait_for("Default controls")?;
        assert!(!app.screen().matches("id:browse-suggestions")?);
    }
    drop(app);
    let errors = ui::test_log::take("retained-search-toolbar");
    assert!(errors.is_empty(), "retained page errors: {errors:?}");
    Ok(())
}
