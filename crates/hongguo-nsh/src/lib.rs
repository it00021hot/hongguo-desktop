mod appearance;
mod bootstrap;
mod boss_key;
mod bridge;
mod emoji;
mod events;
mod file_access;
mod i18n;
mod image_url;
mod library;
mod media;
mod native_window;
mod navigation;
mod notifications;
mod pages;
mod personal_sources;
mod player;
mod preview;
mod shortcuts;
mod typography_preview;
mod ui;
mod update;
mod view_memory;
mod window_modes;

pub use bootstrap::configure_mobile_data_root;
pub use preview::LayoutPreview;
pub use preview::UiPreview;
pub use typography_preview::TypographyPreview;

use i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

#[native_module("hongguo/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
    registry.register::<UiPreview>();
    registry.register::<LayoutPreview>();
    registry.register::<TypographyPreview>();
}

#[native_components(macos)]
fn native_components(components: &nsh_platform_macos::Components) {
    notifications::register(components);
    hongguo_nsh_video_macos::register(components);
    boss_key::register(components);
    player::navigation::register(components);
    window_modes::register(components);
}

#[native_components(windows)]
fn native_components(components: &nsh_platform_windows::Components) {
    notifications::register(components);
    hongguo_nsh_video_windows::register(components);
    boss_key::register(components);
    player::navigation::register(components);
    window_modes::register(components);
}

#[native_components(ios)]
fn native_components(components: &nsh_platform_ios::Components) {
    notifications::register(components);
    hongguo_nsh_video_ios::register(components);
    window_modes::register(components);
    file_access::register(components);
}

#[component]
/// The Rust application root shared by the desktop and mobile hosts.
pub fn App() -> impl IntoElement {
    view_memory::provide();
    set_tokens(
        nsh_base::tokens_of(Some(include_str!("../theme.json"))).expect("validated NSH theme"),
    );
    let session = resource(|| Some(()), |_, _| bridge::request(bootstrap::open()));
    view()
        .flex_1()
        .p_safe_area()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(when(
            move || session.loading(),
            move || ui::status(move || tr("common.loading")),
        ))
        .child(when(
            move || session.error().is_some(),
            move || {
                pages::error_state(
                    move || {
                        session
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || session.refetch(),
                )
            },
        ))
        .child(when(
            move || session.with(|session| session.is_some()),
            move || {
                let notifications = notifications::install();
                let handle = events::SessionHandle::new(session.get().expect("loaded session"));
                let boss_key =
                    boss_key::install(handle.with(|session| session.state.settings().boss_key));
                let window_modes = window_modes::install();
                let file_access = file_access::install();
                let appearance = handle.with(|session| session.state.settings().theme);
                set_color_scheme(match appearance.as_str() {
                    "light" => nsh_core::platform::Appearance::Light,
                    "dark" => nsh_core::platform::Appearance::Dark,
                    _ => nsh_core::platform::Appearance::System,
                });
                let nav = navigation::Navigation::new();
                view_memory::provide_models();
                shortcuts::Shortcuts::provide();
                ui::provide_account_label(
                    computed(move || {
                        handle.identity();
                        handle.with(|session| {
                            session
                                .state
                                .settings()
                                .account
                                .map(|account| account.user_name.trim().to_owned())
                                .filter(|name| !name.is_empty())
                                .map_or_else(|| tr("nav.accountFallback"), SharedString::from)
                        })
                    }),
                    computed(move || {
                        handle.identity();
                        handle.with(|session| session.state.settings().account.is_some())
                    }),
                );
                let theme_pending = signal(false);
                let account_dialogs = pages::account_dialogs(handle);
                ui::shell_with_theme(
                    nav,
                    view(),
                    ui::stack().child(each(
                        nav.routes(),
                        |route: &navigation::Route| format!("{route:?}"),
                        move |route, index| {
                            pages::render(route.get_untracked(), handle, nav)
                                .on_dismiss(move || nav.dismiss(index.get_untracked()))
                        },
                    )),
                    theme_pending,
                    move || {
                        theme_pending.get();
                        handle.with(|session| session.state.settings().theme)
                    },
                    move |appearance| appearance::save(handle, appearance, theme_pending),
                )
                .child(account_dialogs)
                .child(boss_key)
                .child(window_modes)
                .child(file_access)
                .child(notifications)
            },
        ))
}
