use super::*;
use nsh_core::{
    Command, StatusBarStyle,
    platform::{Appearance, Os},
};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Contrast() -> impl IntoElement {
    let nav = Navigation::new();
    set_color_scheme(Appearance::Dark);
    view().flex_1().child(stack().child(each(
        nav.routes(),
        |route: &Route| format!("{route:?}"),
        move |_, index| {
            page(nav, "Contrast")
                .child(text(move || {
                    format!("dark:{} depth:{}", dark(), nav.routes().with(Vec::len))
                }))
                .child(
                    button("Light theme", ButtonVariant::Quiet)
                        .on_press(|| set_color_scheme(Appearance::Light)),
                )
                .child(
                    button("Dark theme", ButtonVariant::Quiet)
                        .on_press(|| set_color_scheme(Appearance::Dark)),
                )
                .child(
                    button("Next page", ButtonVariant::Quiet)
                        .on_press(move || nav.push(Route::Detail("contrast".into()))),
                )
                .on_dismiss(move || nav.dismiss(index.get_untracked()))
        },
    )))
}

fn latest(app: &Headless) -> Option<StatusBarStyle> {
    app.screen()
        .commands()
        .iter()
        .rev()
        .find_map(|(_, command)| match command {
            Command::StatusBar(config) => Some(config.style),
            _ => None,
        })
}

fn start(os: Os) -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<Contrast>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.environment.os = os;
    config.environment.reduce_motion = true;
    Headless::start(config).map_err(|error| error.to_string())
}

#[test]
fn android_status_bar_content_follows_the_live_theme_across_page_navigation() -> Result<(), String>
{
    let mut app = start(Os::Android)?;
    app.wait_for("text:dark:true depth:1")?;
    assert_eq!(latest(&app), Some(StatusBarStyle::Light));
    app.press("Light theme")?;
    app.wait_for("text:dark:false depth:1")?;
    assert_eq!(latest(&app), Some(StatusBarStyle::Dark));
    app.press("Next page")?;
    app.wait_for("text:dark:false depth:2")?;
    assert_eq!(latest(&app), Some(StatusBarStyle::Dark));
    app.press("Dark theme")?;
    app.wait_for("text:dark:true depth:2")?;
    assert_eq!(latest(&app), Some(StatusBarStyle::Light));
    app.back()?;
    app.wait_for("text:dark:true depth:1")?;
    assert_eq!(latest(&app), Some(StatusBarStyle::Light));
    Ok(())
}

#[test]
fn desktop_pages_do_not_override_the_system_status_bar() -> Result<(), String> {
    let mut app = start(Os::MacOs)?;
    app.wait_for("text:dark:true depth:1")?;
    app.press("Light theme")?;
    app.wait_for("text:dark:false depth:1")?;
    assert!(latest(&app).is_none_or(|style| style == StatusBarStyle::Default));
    Ok(())
}
