use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Deserialize)]
struct Tab {
    title: String,
    icon: Option<String>,
}

#[component]
fn SystemTabs() -> impl IntoElement {
    view()
        .flex_1()
        .child(sections(Navigation::new()).h(px(80.0)))
        .child(text("Tabs mounted"))
}

fn tabs(os: nsh_core::platform::Os) -> Result<Vec<Tab>, String> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observed = seen.clone();
    let mut config = EngineConfig::new(Root::of::<SystemTabs>());
    config.environment.os = os;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let nsh_core::MountItem::ComponentProps { props, .. } = item {
                for (key, value) in props {
                    if key.as_ref() == "items"
                        && let Some(value) = value
                    {
                        *observed.lock().unwrap() = serde_json::from_value(value.clone())
                            .expect("native tab item contract");
                    }
                }
            }
        }
    });
    app.wait_for("Tabs mounted")?;
    let result = seen.lock().unwrap().clone();
    Ok(result)
}

#[test]
fn system_tabs_send_an_explicit_platform_icon_for_each_destination() -> Result<(), String> {
    for os in [nsh_core::platform::Os::Ios, nsh_core::platform::Os::Android] {
        let tabs = tabs(os)?;
        assert_eq!(tabs.len(), MOBILE_SECTIONS.len());
        assert!(tabs.iter().all(|tab| !tab.title.is_empty()));
        assert!(
            tabs.iter()
                .all(|tab| tab.icon.as_ref().is_some_and(|icon| !icon.is_empty())),
            "{os:?} native tab icons: {tabs:?}"
        );
    }
    Ok(())
}

#[component]
fn MobileDestinations() -> impl IntoElement {
    let nav = Navigation::new();
    nav.push(Route::Settings);
    let pages = stack().child(each(
        nav.routes(),
        |route: &Route| format!("{route:?}"),
        move |route, index| {
            let route = route.get_untracked();
            page(nav, format!("{route:?}"))
                .child(text(move || {
                    nav.routes().with(|routes| {
                        format!("route:{:?} depth:{}", routes.last().unwrap(), routes.len())
                    })
                }))
                .on_dismiss(move || nav.dismiss(index.get_untracked()))
        },
    ));
    shell(nav, view(), pages)
}

#[test]
fn mobile_main_menu_reaches_every_extra_section_and_back_closes_the_sheet_first()
-> Result<(), String> {
    for os in [nsh_core::platform::Os::Ios, nsh_core::platform::Os::Android] {
        let root = Root::of::<MobileDestinations>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap());
        let mut config = EngineConfig::new(root);
        config.environment.os = os;
        config.environment.reduce_motion = true;
        config.viewport = nsh_engine::Size::new(px(480.0), px(840.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("text:route:Settings depth:2")?;
        let opener = app
            .screen()
            .press_target("role:button[Main menu]")?
            .expect("real header button");
        app.press("role:button[Main menu]")?;
        app.wait_for("id:mobile-main-menu")?;
        let before_close = app.screen().commands().len();
        app.back()?;
        app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
        app.wait_for("text:route:Settings depth:2")?;
        assert!(
            app.screen().commands()[before_close..]
                .iter()
                .any(|(id, command)| *id == opener && matches!(command, nsh_core::Command::Focus)),
            "sheet dismissal must focus its actual header opener"
        );
        app.press("role:button[Main menu]")?;
        app.wait_for("id:mobile-main-menu")?;
        app.press_key("escape")?;
        app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
        app.wait_for("text:route:Settings depth:2")?;
        for (label, route, scroll) in [
            ("New", "New", false),
            ("Collections", "Collections", false),
            ("Liked", "Liked", false),
            ("Following", "Follows", false),
            ("Reservations", "Reservations", true),
            ("Tasks", "Tasks", true),
            ("Merge", "Merge", true),
            ("Storage", "Storage", true),
        ] {
            app.press("role:button[Main menu]")?;
            app.wait_for("id:mobile-main-menu")?;
            assert_eq!(
                app.screen().count("id:mobile-main-menu >> role:button")?,
                DESKTOP_SECTIONS.len()
            );
            if scroll {
                app.scroll("id:mobile-menu-scroll", px(1000.0))?;
            }
            app.press(&format!("id:mobile-main-menu >> role:button[{label}]"))?;
            app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
            app.wait_for(&format!("text:route:{route} depth:3"))?;
            app.back()?;
            app.wait_for("text:route:Settings depth:2")?;
        }
    }
    Ok(())
}

#[component]
fn MobileAppearance() -> impl IntoElement {
    let nav = Navigation::new();
    let appearance = signal(nsh_core::platform::Appearance::Dark);
    let writes = signal(0_u32);
    let busy = signal(false);
    set_color_scheme(nsh_core::platform::Appearance::Dark);
    let content = stack().child(dynamic(move || {
        page(nav, "Preferences fixture")
            .child(text(move || {
                format!(
                    "theme:{:?} callbacks:{} dark:{}",
                    appearance.get(),
                    writes.get(),
                    dark()
                )
            }))
            .child(text(move || format!("home:{}", tr("nav.home.title"))))
            .child(
                button("Finish theme write", ButtonVariant::Quiet)
                    .on_press(move || busy.set(false)),
            )
    }));
    shell_with_theme(
        nav,
        view(),
        content,
        busy,
        move || {
            match appearance.get() {
                nsh_core::platform::Appearance::Light => "light",
                nsh_core::platform::Appearance::Dark => "dark",
                nsh_core::platform::Appearance::System => "auto",
            }
            .to_owned()
        },
        move |next| {
            busy.set(true);
            writes.update(|writes| *writes += 1);
            appearance.set(next);
            set_color_scheme(next);
        },
    )
}

#[test]
fn mobile_menu_preferences_reuse_live_theme_and_language_controls_and_restore_the_header_opener()
-> Result<(), String> {
    for os in [nsh_core::platform::Os::Ios, nsh_core::platform::Os::Android] {
        let root = Root::of::<MobileAppearance>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap());
        let mut config = EngineConfig::new(root);
        config.environment.os = os;
        config.environment.reduce_motion = true;
        config.viewport = nsh_engine::Size::new(px(480.0), px(840.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("text:theme:Dark callbacks:0 dark:true")?;
        let opener = app
            .screen()
            .press_target("role:button[Main menu]")?
            .unwrap();
        app.press("role:button[Main menu]")?;
        app.wait_for("role:combobox[Toggle theme]")?;
        app.press("role:combobox[Toggle theme]")?;
        app.press("role:option[Light]")?;
        app.wait_for("text:theme:Light callbacks:1 dark:false")?;
        assert!(!app.screen().enabled("role:combobox[Toggle theme]")?);
        assert!(app.screen().matches("id:mobile-main-menu")?);
        let before_close = app.screen().commands().len();
        app.back()?;
        app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
        assert!(
            app.screen().commands()[before_close..]
                .iter()
                .any(|(id, command)| *id == opener && matches!(command, nsh_core::Command::Focus))
        );
        app.press("Finish theme write")?;
        app.press("role:button[Main menu]")?;
        app.wait_for("role:combobox[Toggle theme] >> text:Light")?;
        app.press("role:combobox[Language]")?;
        app.press("role:option[Simplified Chinese]")?;
        app.wait_for("role:button[关闭]")?;
        app.wait_for("text:home:推荐")?;
        app.press("role:combobox[语言]")?;
        app.press("role:option[English]")?;
        app.wait_for("role:button[Close]")?;
        app.wait_for("text:home:Discover")?;
        app.press("role:combobox[Toggle theme]")?;
        app.back()?;
        assert!(app.screen().matches("id:mobile-main-menu")?);
        assert!(!app.screen().matches("role:option[Dark]")?);
        app.wait_for("text:theme:Light callbacks:1 dark:false")?;
        app.back()?;
        app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
    }
    Ok(())
}
