use super::*;
use crate::player::navigation::{Controller, FeedNavigation, State};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn BingeHistory() -> impl IntoElement {
    let nav = Navigation::new();
    let feed = FeedNavigation::new(|_| true);
    let active = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&Route::Catalog))
    });
    let player = Controller::new(
        State {
            active,
            blocked: computed(|| false),
            series: computed(|| "series".into()),
        },
        Some(feed),
        |_| {},
    );
    let selected = player.in_binge();
    nav.on_back(
        computed(move || active.get() && selected.get()),
        move || player.exit_binge(),
    );
    let content = view()
        .child(text(move || format!("selected:{}", selected.get())))
        .child(
            crate::ui::button("Select drama", crate::ui::ButtonVariant::Quiet)
                .on_press(move || player.enter_binge()),
        )
        .child(
            crate::ui::button("Open history", crate::ui::ButtonVariant::Quiet)
                .on_press(move || nav.push(Route::History)),
        )
        .child(text(move || {
            format!("routes:{}", nav.routes().with(Vec::len))
        }));
    crate::ui::shell(nav, view(), content)
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<BingeHistory>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json")))?),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    Headless::start(config).map_err(|error| error.to_string())
}

#[test]
fn back_exits_selected_drama_without_changing_the_page() -> Result<(), String> {
    let mut app = start()?;
    app.wait_for("text:selected:false")?;
    assert!(!app.screen().enabled("Back")?);
    app.press("Select drama")?;
    app.wait_for("text:selected:true")?;
    assert!(app.screen().enabled("Back")?);
    app.press("Back")?;
    app.wait_for("text:selected:false")?;
    app.wait_for("text:routes:1")?;
    assert!(!app.screen().enabled("Back")?);
    Ok(())
}

#[test]
fn a_hidden_selected_drama_does_not_intercept_another_pages_back() -> Result<(), String> {
    let mut app = start()?;
    app.wait_for("text:selected:false")?;
    app.press("Select drama")?;
    app.press("Open history")?;
    app.wait_for("text:routes:2")?;
    app.press("Back")?;
    app.wait_for("text:routes:1")?;
    app.wait_for("text:selected:true")?;
    app.press("Back")?;
    app.wait_for("text:selected:false")?;
    Ok(())
}
