use super::{ButtonVariant, Icon, button, icon};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
};
use nsh_base::theme;
use nsh_core::prelude::*;
mod mobile_catalog;
mod mobile_menu;
mod mobile_preferences;
pub(crate) use mobile_catalog::{mobile_catalog_header, mobile_channel_tab};
#[cfg(test)]
mod mobile_layout_tests;
#[cfg(test)]
mod status_bar_tests;
#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct AccountLabel {
    label: Memo<SharedString>,
    logged_in: Memo<bool>,
}

pub(crate) fn provide_account_label(label: Memo<SharedString>, logged_in: Memo<bool>) {
    provide_context(AccountLabel { label, logged_in });
}

struct MobileSection {
    title: &'static str,
    route: Route,
    ios_icon: &'static str,
    android_icon: &'static str,
}

const MOBILE_SECTIONS: [MobileSection; 5] = [
    MobileSection {
        title: "nav.home.title",
        route: Route::Catalog,
        ios_icon: "flame",
        android_icon: "hg_nav_recommend",
    },
    MobileSection {
        title: "nav.browse.title",
        route: Route::Browse,
        ios_icon: "safari",
        android_icon: "hg_nav_browse",
    },
    MobileSection {
        title: "nav.rank.title",
        route: Route::Rank,
        ios_icon: "trophy",
        android_icon: "hg_nav_rank",
    },
    MobileSection {
        title: "nav.history.title",
        route: Route::History,
        ios_icon: "clock.arrow.circlepath",
        android_icon: "hg_nav_history",
    },
    MobileSection {
        title: "nav.settings.title",
        route: Route::Settings,
        ios_icon: "gearshape",
        android_icon: "hg_nav_settings",
    },
];
const DESKTOP_SECTIONS: [(&str, Icon, Route); 13] = [
    ("nav.home.title", Icon::Home, Route::Catalog),
    ("nav.browse.title", Icon::Browse, Route::Browse),
    ("nav.rank.title", Icon::Rank, Route::Rank),
    ("nav.new.title", Icon::New, Route::New),
    ("nav.history.title", Icon::History, Route::History),
    (
        "nav.collections.title",
        Icon::Collections,
        Route::Collections,
    ),
    ("nav.liked.title", Icon::Liked, Route::Liked),
    ("nav.follows.title", Icon::Follows, Route::Follows),
    (
        "nav.reservations.title",
        Icon::Reservations,
        Route::Reservations,
    ),
    ("nav.tasks.title", Icon::Tasks, Route::Tasks),
    ("nav.merge.title", Icon::Merge, Route::Merge),
    ("nav.storage.title", Icon::Storage, Route::Storage),
    ("nav.settings.title", Icon::Settings, Route::Settings),
];

pub(crate) fn page(nav: Navigation, title: impl IntoReactive<SharedString> + Clone) -> Screen {
    let header = view()
        .h_11()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .bg(move || theme().color("titlebar").expect("titlebar token"))
        .border_b_1()
        .border_color(move || theme().border)
        .child(when(
            move || nav.can_back(),
            move || {
                button("", ButtonVariant::Quiet)
                    .accessibility_label(move || tr("nav.back"))
                    .min_h(px(28.0))
                    .size(px(28.0))
                    .p_0()
                    .child(icon(Icon::Back))
                    .on_press(move || nav.back())
            },
        ))
        .child(
            text(title.clone())
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        );
    let page = nsh_base::nav_page()
        .title(title)
        .header(header.style_when(is_desktop, |style| style.hidden()))
        .header_style_with(|style| {
            style
                .bg(move || theme().background)
                .text_color(move || theme().foreground)
                .tint_color(move || theme().foreground)
        })
        .bg(move || theme().background)
        // NSH's native navigation backdrop uses the system window color.
        // Paint a content child above it so user-selected themes stay exact.
        .child(
            view()
                .absolute()
                .inset_0()
                .bg(move || theme().background)
                .pointer_events(nsh_core::PointerEvents::None),
        );
    let page = if os() == nsh_core::platform::Os::Android {
        page.status_bar(|| {
            nsh_core::StatusBarSetting::default().foreground(if dark() {
                nsh_core::StatusBarStyle::Light
            } else {
                nsh_core::StatusBarStyle::Dark
            })
        })
    } else {
        page
    };
    if !is_desktop()
        && let Some(menu) = mobile_menu::MainMenu::current()
    {
        page.header_right(menu.trigger())
            .header_right_symbol("line.3.horizontal")
    } else {
        page
    }
}

pub(crate) fn stack() -> Stack {
    nsh_base::nav_stack().flex_1().min_w(px(0.0))
}

pub(crate) fn sections(nav: Navigation) -> View {
    if matches!(
        os(),
        nsh_core::platform::Os::Ios | nsh_core::platform::Os::Android
    ) {
        return view().test_id("main-nav").flex_shrink_0().child(
            nsh_core::mobile::tab_bar()
                .flex_shrink_0()
                .items(move || {
                    MOBILE_SECTIONS
                        .iter()
                        .map(|section| {
                            nsh_core::mobile::TabBarItem::new(tr(section.title)).icon(
                                if os() == nsh_core::platform::Os::Ios {
                                    section.ios_icon
                                } else {
                                    section.android_icon
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .selected_index(move || {
                    let section = if nav.section() == Route::Account {
                        Route::Settings
                    } else {
                        nav.section()
                    };
                    MOBILE_SECTIONS
                        .iter()
                        .position(|item| item.route == section)
                        .unwrap_or(MOBILE_SECTIONS.len()) as u32
                })
                .on_change(move |index: &u32| {
                    if let Some(item) = MOBILE_SECTIONS.get(*index as usize) {
                        nav.push(item.route.clone());
                    }
                }),
        );
    }
    sidebar(nav, signal(false))
}

pub(crate) fn shell(
    nav: Navigation,
    toolbar_content: impl IntoElement,
    content: impl IntoElement,
) -> View {
    let appearance = signal(nsh_core::platform::Appearance::System);
    shell_with_theme(
        nav,
        toolbar_content,
        content,
        false,
        move || {
            match appearance.get() {
                nsh_core::platform::Appearance::System => "auto",
                nsh_core::platform::Appearance::Light => "light",
                nsh_core::platform::Appearance::Dark => "dark",
            }
            .into()
        },
        move |value| {
            appearance.set(value);
            set_color_scheme(value);
        },
    )
}

pub(crate) fn shell_with_theme(
    nav: Navigation,
    toolbar_content: impl IntoElement,
    content: impl IntoElement,
    theme_busy: impl IntoReactive<bool>,
    theme_value: impl Fn() -> String + 'static,
    on_theme: impl Fn(nsh_core::platform::Appearance) + 'static,
) -> View {
    let theme_busy = theme_busy.into_reactive();
    let language = crate::i18n::provide_language();
    let toolbar = super::toolbar::ToolbarSlot::provide();
    if !is_desktop() {
        let menu = mobile_menu::MainMenu::provide(nav);
        let preferences =
            mobile_preferences::Preferences::new(theme_busy, theme_value, on_theme, language);
        return view()
            .flex_1()
            .pb(move || {
                if mobile_tabs_visible(nav) {
                    px(0.0)
                } else {
                    safe_area().bottom
                }
            })
            .child(content)
            .child(when(
                move || mobile_tabs_visible(nav),
                move || sections(nav),
            ))
            .child(menu.sheet(preferences));
    }
    crate::native_window::install();
    let collapsed = signal(false);
    let pinned = signal(false);
    let is_pinned = move || {
        crate::window_modes::WindowModes::current()
            .map_or_else(|| pinned.get(), |modes| modes.state().pinned)
    };
    view()
        .flex_1()
        .child(
            view()
                .h_11()
                .test_id("app-titlebar")
                .style_when(move || chrome_hidden(nav), |style| style.hidden())
                .window_drag(true)
                .flex_row()
                .items_center()
                .gap_3()
                .pr(crate::native_window::trailing_inset)
                .pl(crate::native_window::leading_inset)
                .bg(move || theme().color("titlebar").expect("titlebar token"))
                .border_b_1()
                .border_color(move || theme().border)
                .child(
                    view()
                        .flex_row()
                        .gap_0p5()
                        .child(
                            toolbar_button(Icon::Back, "nav.back")
                                .disabled(move || !nav.can_back())
                                .on_press(move || nav.back()),
                        )
                        .child(
                            toolbar_button(Icon::Forward, "nav.forward")
                                .disabled(move || !nav.can_forward())
                                .on_press(move || nav.forward()),
                        )
                        .child(
                            toolbar_button(Icon::Sidebar, "nav.collapse")
                                .style_when(
                                    move || window().width.0 < 768.0,
                                    |style| style.hidden(),
                                )
                                .accessibility_label(move || {
                                    tr(if collapsed.get() {
                                        "nav.expand"
                                    } else {
                                        "nav.collapse"
                                    })
                                })
                                .on_press(move || collapsed.update(|value| *value = !*value)),
                        ),
                )
                .child(
                    view()
                        .absolute()
                        .inset_0()
                        .flex_row()
                        .justify_center()
                        .items_center()
                        .pointer_events(nsh_core::PointerEvents::BoxNone)
                        .style_when(move || portrait_mode(nav), |style| style.hidden())
                        .child(
                            view()
                                .style_when(move || toolbar.occupied(), |style| style.hidden())
                                .child(toolbar_content),
                        )
                        .child(dynamic(move || toolbar.content())),
                )
                .child(
                    view()
                        .test_id("app-toolbar-rest")
                        .absolute()
                        .left(px(180.0))
                        .top_0()
                        .w_8()
                        .h_11(),
                )
                .child(view().flex_1())
                .child(
                    view()
                        .flex_row()
                        .gap_1()
                        .style_when(move || window().width.0 < 768.0, |style| style.hidden())
                        .child(
                            super::controls::surface(ButtonVariant::Quiet)
                                .accessibility_label(move || {
                                    tr(if is_pinned() {
                                        "player.unpin"
                                    } else {
                                        "player.pin"
                                    })
                                })
                                .min_h(px(28.0))
                                .size(px(28.0))
                                .p_0()
                                .selected(is_pinned)
                                .selected_style(|style| {
                                    style
                                        .bg(move || theme().background.opacity(0.0))
                                        .border(px(0.0))
                                        .text_color(move || theme().foreground)
                                })
                                .child(dynamic(move || {
                                    icon(if is_pinned() {
                                        Icon::PinFilled
                                    } else {
                                        Icon::Pin
                                    })
                                }))
                                .on_press(move || {
                                    if let Some(modes) = crate::window_modes::WindowModes::current()
                                    {
                                        modes.toggle_pinned();
                                    } else {
                                        pinned.update(|value| *value = !*value);
                                    }
                                }),
                        )
                        .child(super::toolbar_menu::menu(
                            toolbar_button(Icon::Languages, "search.langLabel"),
                            super::toolbar_menu::LANGUAGES,
                            move || language.value().to_owned(),
                            move |value| super::toolbar_menu::change_language(language, value),
                        ))
                        .child(super::toolbar_menu::menu(
                            super::controls::surface(ButtonVariant::Quiet)
                                .accessibility_label(move || tr("common.toggleTheme"))
                                .min_h(px(28.0))
                                .size(px(28.0))
                                .p_0()
                                .child(dynamic(move || {
                                    icon(if dark() { Icon::Moon } else { Icon::Sun })
                                }))
                                .disabled(move || theme_busy.get()),
                            super::toolbar_menu::THEMES,
                            theme_value,
                            move |value| {
                                on_theme(match value {
                                    "light" => nsh_core::platform::Appearance::Light,
                                    "dark" => nsh_core::platform::Appearance::Dark,
                                    _ => nsh_core::platform::Appearance::System,
                                })
                            },
                        )),
                ),
        )
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(sidebar(nav, collapsed))
                .child(content),
        )
        .child(when(
            move || window().width.0 < 768.0 && !chrome_hidden(nav) && !portrait_mode(nav),
            move || desktop_bottom_nav(nav),
        ))
}

fn mobile_tabs_visible(nav: Navigation) -> bool {
    !nav.routes()
        .with(|routes| matches!(routes.last(), Some(Route::Player(_, _))))
        && !chrome_hidden(nav)
}

fn desktop_bottom_nav(nav: Navigation) -> View {
    let account = use_context::<AccountLabel>();
    let entries = [
        ("nav.home.title", Icon::Home, Route::Catalog),
        ("nav.browse.title", Icon::Browse, Route::Browse),
        ("nav.rank.title", Icon::Rank, Route::Rank),
        ("nav.history.title", Icon::History, Route::History),
        ("nav.login", Icon::Account, Route::Settings),
    ];
    view()
        .test_id("bottom-nav")
        .h(px(50.0))
        .flex_shrink_0()
        .flex_row()
        .items_stretch()
        .bg(move || theme().color("sidebar").expect("sidebar token"))
        .border_t_1()
        .border_color(move || theme().border)
        .children(entries.into_iter().map(move |(key, glyph, route)| {
            let selected_route = route.clone();
            let label = move || {
                tr(
                    if key == "nav.login" && account.is_some_and(|account| account.logged_in.get())
                    {
                        "nav.settings.title"
                    } else {
                        key
                    },
                )
            };
            nsh_base::button()
                .accessibility_label(label)
                .flex_1()
                .min_w(px(0.0))
                .h(px(49.0))
                .min_h(px(0.0))
                .py_1p5()
                .gap_0p5()
                .items_center()
                .justify_center()
                .text_color(move || theme().foreground.opacity(0.7))
                .selected(move || nav.is_selected(&selected_route))
                .selected_style(|style| style.text_color(move || theme().primary))
                .hover(|style| {
                    style
                        .bg(move || theme().accent)
                        .text_color(move || theme().foreground)
                })
                .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
                .child(icon(glyph).size_5())
                .child(
                    text(label)
                        .text_size(px(10.0))
                        .line_height(px(15.0))
                        .font_weight(FontWeight::NORMAL),
                )
                .on_press(move || nav.push(route.clone()))
        }))
}

fn toolbar_button(glyph: Icon, label: &'static str) -> nsh_base::ButtonElement {
    super::controls::surface(ButtonVariant::Quiet)
        .accessibility_label(move || tr(label))
        .min_h(px(28.0))
        .size(px(28.0))
        .p_0()
        .child(icon(glyph))
}

fn sidebar(nav: Navigation, collapsed: Signal<bool>) -> View {
    let compact = move || collapsed.get();
    view()
        .test_id("main-nav")
        .style_when(
            move || window().width.0 < 768.0 || chrome_hidden(nav) || portrait_mode(nav),
            |style| style.hidden(),
        )
        .w(move || px(if compact() { 56.0 } else { 224.0 }))
        .h_full()
        .transition_all()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .bg(move || theme().color("sidebar").expect("sidebar token"))
        .border_r_1()
        .border_color(move || theme().border)
        .child(
            scroll().flex_1().child(
                view().gap_0p5().p_2().children(
                    DESKTOP_SECTIONS
                        .into_iter()
                        .map(move |(key, glyph, route)| nav_item(nav, key, glyph, route, compact)),
                ),
            ),
        )
        .child(
            view()
                .p_2()
                .border_t_1()
                .border_color(move || theme().border)
                .child(nav_item(
                    nav,
                    "nav.accountFallback",
                    Icon::Account,
                    Route::Account,
                    compact,
                )),
        )
}

fn player_surface(nav: Navigation) -> bool {
    nav.routes()
        .with(|routes| matches!(routes.last(), Some(Route::Catalog | Route::Player(_, _))))
}
fn portrait_mode(nav: Navigation) -> bool {
    player_surface(nav)
        && crate::window_modes::WindowModes::current()
            .is_some_and(|modes| modes.state().mode == crate::window_modes::Mode::Portrait)
}
fn chrome_hidden(nav: Navigation) -> bool {
    player_surface(nav)
        && crate::window_modes::WindowModes::current().is_some_and(|modes| {
            let state = modes.state();
            state.mode == crate::window_modes::Mode::Mini
                || state.frameless
                || state.fullscreen
                || state.landscape
        })
}

fn nav_item(
    nav: Navigation,
    key: &'static str,
    glyph: Icon,
    route: Route,
    compact: impl Fn() -> bool + Copy + 'static,
) -> nsh_base::ButtonElement {
    let selected = route.clone();
    let font_selected = selected.clone();
    let account = (key == "nav.accountFallback")
        .then(use_context::<AccountLabel>)
        .flatten();
    let label = move || {
        account
            .map(|account| account.label.get())
            .unwrap_or_else(|| tr(key))
    };
    super::controls::surface(ButtonVariant::Quiet)
        .accessibility_label(label)
        .h(px(37.0))
        .min_h(px(37.0))
        .px_3()
        .py_2()
        .flex_row()
        .justify_start()
        .gap_3()
        .text_color(move || {
            theme()
                .color("sidebar-foreground")
                .expect("sidebar foreground token")
        })
        .selected(move || nav.is_selected(&selected))
        .selected_style(|style| {
            style
                .bg(move || {
                    theme()
                        .color("sidebar-selected")
                        .expect("sidebar selected token")
                })
                .text_color(move || {
                    theme()
                        .color("sidebar-selected-foreground")
                        .expect("sidebar selected foreground token")
                })
        })
        .child(icon(glyph))
        .child(when(
            move || !compact(),
            move || {
                let font_selected = font_selected.clone();
                text(label)
                    .text_sm()
                    .line_height(px(21.0))
                    .font_weight(FontWeight::NORMAL)
                    .style_when(
                        move || nav.is_selected(&font_selected),
                        |style| style.font_weight(FontWeight::MEDIUM),
                    )
                    .number_of_lines(1)
            },
        ))
        .on_press(move || {
            if account.is_some_and(|account| account.logged_in.get_untracked()) {
                nav.push(Route::Settings);
            } else {
                nav.push(route.clone());
            }
        })
}
