use crate::{
    navigation::{Navigation, Route},
    ui::{self, ButtonVariant},
};
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root, Size};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn Pages() -> impl IntoElement {
    super::provide();
    let nav = Navigation::new();
    view()
        .flex_1()
        .child(
            ui::button("Open browse", ButtonVariant::Quiet)
                .on_press(move || nav.push(Route::Browse)),
        )
        .child(
            ui::button("Open rank", ButtonVariant::Quiet).on_press(move || nav.push(Route::Rank)),
        )
        .child(ui::button("Go back", ButtonVariant::Quiet).on_press(move || nav.back()))
        .child(ui::button("Go forward", ButtonVariant::Quiet).on_press(move || nav.forward()))
        .child(ui::stack().child(each(
            nav.routes(),
            |route: &Route| format!("{route:?}"),
            move |route, _| match route.get_untracked() {
                Route::Browse => crate::pages::preview_browse(nav),
                Route::Rank => crate::pages::preview_rank(nav),
                _ => ui::page(nav, "Empty test route"),
            },
        )))
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<Pages>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
    );
    config.viewport = Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Open browse")?;
    Ok(app)
}

#[test]
fn browse_draft_and_filters_survive_a_real_back_and_forward_remount() -> Result<(), String> {
    let mut app = start()?;
    app.press("Open browse")?;
    app.wait_for("id:browse-query")?;
    app.press("role:button[真人剧]")?;
    app.wait_for("role:button[Filters] >> text:1")?;
    app.type_text("id:browse-query", "找回草稿")?;
    app.press("Go back")?;
    app.wait_for_gone("id:browse-query", Duration::from_secs(1))?;
    app.press("Go forward")?;
    app.wait_for("id:browse-query")?;
    assert_eq!(
        app.screen().input_text("id:browse-query")?,
        Some("找回草稿")
    );
    app.wait_for("role:button[Filters] >> text:1")?;
    Ok(())
}

#[test]
fn rank_sub_list_survives_back_and_forward_in_the_production_presenter() -> Result<(), String> {
    let mut app = start()?;
    app.press("Open rank")?;
    app.wait_for("role:button[推荐榜]")?;
    app.press("role:button[推荐榜]")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:button[推荐榜]")?
            .pressed,
        Some(nsh_core::Checked::True)
    );
    app.press("Go back")?;
    app.wait_for_gone("role:button[推荐榜]", Duration::from_secs(1))?;
    app.press("Go forward")?;
    app.wait_for("role:button[推荐榜]")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:button[推荐榜]")?
            .pressed,
        Some(nsh_core::Checked::True)
    );
    Ok(())
}

#[test]
fn rank_main_scroll_survives_remount_and_resets_when_sublist_changes() -> Result<(), String> {
    let mut app = start()?;
    app.press("Open rank")?;
    app.wait_for("id:rank-results")?;
    app.scroll("id:rank-results", px(400.0))?;
    app.press("Go back")?;
    app.wait_for_gone("id:rank-results", Duration::from_secs(1))?;
    app.press("Go forward")?;
    app.wait_for("id:rank-results")?;
    app.scroll("id:rank-results", px(-400.0))?;
    assert!(app.scroll("id:rank-results", px(-1.0)).is_err());
    app.scroll("id:rank-results", px(400.0))?;
    app.press("role:button[推荐榜]")?;
    app.wait(Duration::from_millis(32))?;
    assert!(app.scroll("id:rank-results", px(-1.0)).is_err());
    Ok(())
}

#[test]
fn browse_returns_to_its_scrolled_results_after_a_real_route_remount() -> Result<(), String> {
    let mut app = start()?;
    app.set_viewport(Size::new(px(1280.0), px(600.0)))?;
    app.press("Open browse")?;
    app.wait_for("id:browse-results")?;
    app.scroll("id:form-scroll", px(300.0))?;
    app.press("Go back")?;
    app.wait_for_gone("id:browse-results", Duration::from_secs(1))?;
    app.press("Go forward")?;
    app.wait_for("id:browse-results")?;
    app.scroll("id:form-scroll", px(-300.0))?;
    assert!(app.scroll("id:form-scroll", px(-1.0)).is_err());
    Ok(())
}
