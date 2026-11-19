use crate::ui::ButtonVariant;
use crate::ui::Icon;
use crate::ui::icon;
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
};
use nsh_base::theme;
use nsh_core::prelude::*;
mod exit;
mod grouped;
pub(crate) use grouped::header as section_header;
mod mobile_catalog;
mod mobile_menu;
mod mobile_preferences;
mod player_overlay;
mod sidebar_preferences;
pub(crate) use player_overlay::PlayerOverlay;
mod tray;
pub(crate) use mobile_catalog::{mobile_catalog_header, mobile_channel_tab};

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
}

const MOBILE_SECTIONS: [MobileSection; 4] = [
    MobileSection {
        title: "nav.primaryHome",
        route: Route::Catalog,
    },
    MobileSection {
        title: "nav.browse.title",
        route: Route::Browse,
    },
    MobileSection {
        title: "nav.primaryDownloads",
        route: Route::Tasks,
    },
    MobileSection {
        title: "nav.my",
        route: Route::History,
    },
];
const DESKTOP_SECTIONS: [(&str, Icon, Route); 4] = [
    ("nav.primaryHome", Icon::Home, Route::Catalog),
    ("nav.browse.title", Icon::Browse, Route::Browse),
    ("nav.primaryDownloads", Icon::Download, Route::Tasks),
    ("nav.my", Icon::Account, Route::History),
];

pub(crate) fn page(nav: Navigation, title: impl IntoReactive<SharedString> + Clone) -> Screen {
    let header = view()
        .h_11()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .bg(move || theme().color("titlebar").expect("titlebar token"))
        .child(when(
            move || nav.can_back(),
            move || {
                crate::ui::controls::surface(ButtonVariant::Quiet)
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
    let page = if matches!(
        os(),
        nsh_core::platform::Os::Android | nsh_core::platform::Os::Ios
    ) {
        page.status_bar(move || {
            nsh_core::StatusBarSetting::default()
                .hidden(chrome_hidden(nav))
                .foreground(if dark() {
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

pub(crate) fn content(_nav: Navigation, _title: impl IntoReactive<SharedString>) -> View {
    view().flex_1().min_h_0()
}

pub(crate) fn sections(nav: Navigation) -> View {
    if matches!(
        os(),
        nsh_core::platform::Os::Ios | nsh_core::platform::Os::Android
    ) {
        return view()
            .flex_shrink_0()
            .child(
                crate::ui::tabs::mobile_navigation().children(MOBILE_SECTIONS.iter().map(|item| {
                    let selected = item.route.clone();
                    let destination = item.route.clone();
                    crate::ui::tabs::mobile_navigation_item(move || tr(item.title))
                        .selected(move || grouped::primary(nav.section()) == selected)
                        .on_press(move || nav.push(destination.clone()))
                })),
            );
    }
    sidebar(nav, sidebar_preferences::collapsed())
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
    let toolbar = crate::ui::toolbar::ToolbarSlot::provide();
    if !is_desktop() {
        let menu = mobile_menu::MainMenu::provide(nav);
        let preferences = mobile_preferences::Preferences::new(theme_busy, theme_value, on_theme);
        provide_context(preferences);
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
    let exit_requested = signal(false);
    tray::install(exit_requested);
    let collapsed = sidebar_preferences::collapsed();
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
                .style_when(move || chrome_hidden(nav), |style| style.hidden())
                .window_drag(true)
                .flex_row()
                .items_center()
                .gap_3()
                .bg(move || theme().color("titlebar").expect("titlebar token"))
                .child(
                    view().flex_1().min_w_0().child(
                        view()
                            .pl(crate::native_window::leading_inset)
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
                                    .on_press(move || sidebar_preferences::toggle(collapsed)),
                            ),
                    ),
                )
                .child(
                    view()
                        .flex_shrink_0()
                        .min_w_0()
                        .flex_row()
                        .justify_center()
                        .items_center()
                        .pointer_events(nsh_core::PointerEvents::BoxNone)
                        .style_when(move || portrait_mode(nav), |style| style.hidden())
                        .child(dynamic(move || toolbar.content())),
                )
                .child(
                    view().flex_1().min_w_0().child(
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap_3()
                            .pr(crate::native_window::trailing_inset)
                            .child(
                                view()
                                    .flex_1()
                                    .min_w_0()
                                    .max_w(px(208.0))
                                    .child(toolbar_content),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .flex_shrink_0()
                                    .gap_1()
                                    .style_when(
                                        move || window().width.0 < 768.0,
                                        |style| style.hidden(),
                                    )
                                    .child(
                                        crate::ui::controls::surface(ButtonVariant::Quiet)
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
                                                if let Some(modes) =
                                                    crate::window_modes::WindowModes::current()
                                                {
                                                    modes.toggle_pinned();
                                                } else {
                                                    pinned.update(|value| *value = !*value);
                                                }
                                            }),
                                    )
                                    .child(crate::ui::toolbar_menu::menu(
                                        toolbar_button(Icon::Languages, "search.langLabel"),
                                        crate::ui::toolbar_menu::LANGUAGES,
                                        move || crate::i18n::language().to_owned(),
                                        crate::ui::toolbar_menu::change_language,
                                    ))
                                    .child(crate::ui::toolbar_menu::menu(
                                        crate::ui::controls::surface(ButtonVariant::Quiet)
                                            .accessibility_label(move || tr("common.toggleTheme"))
                                            .min_h(px(28.0))
                                            .size(px(28.0))
                                            .p_0()
                                            .child(dynamic(move || {
                                                icon(if dark() { Icon::Moon } else { Icon::Sun })
                                            }))
                                            .disabled(move || theme_busy.get()),
                                        crate::ui::toolbar_menu::THEMES,
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
                    ),
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
        .child(exit::confirmation(exit_requested))
}

fn mobile_tabs_visible(nav: Navigation) -> bool {
    !PlayerOverlay::comments_open()
        && !nav
            .routes()
            .with(|routes| matches!(routes.last(), Some(Route::Player(_, _))))
        && !chrome_hidden(nav)
}

fn desktop_bottom_nav(nav: Navigation) -> View {
    view()
        .h(px(50.0))
        .flex_shrink_0()
        .flex_row()
        .items_stretch()
        .bg(move || theme().background)
        .border_t_1()
        .border_color(move || theme().border)
        .children(
            DESKTOP_SECTIONS
                .into_iter()
                .map(move |(key, glyph, route)| {
                    let selected_route = route.clone();
                    let label = move || tr(key);
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
                        .selected(move || grouped::primary(nav.section()) == selected_route)
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
                }),
        )
}

fn toolbar_button(glyph: Icon, label: &'static str) -> nsh_base::ButtonElement {
    crate::ui::controls::surface(ButtonVariant::Quiet)
        .accessibility_label(move || tr(label))
        .min_h(px(28.0))
        .size(px(28.0))
        .p_0()
        .child(icon(glyph))
        .child(crate::ui::hint(move || tr(label), Placement::Bottom))
}

fn sidebar(nav: Navigation, collapsed: Signal<bool>) -> View {
    let compact = move || collapsed.get();
    view()
        .style_when(
            move || window().width.0 < 768.0 || chrome_hidden(nav) || portrait_mode(nav),
            |style| style.hidden(),
        )
        .w(move || px(if compact() { 56.0 } else { 148.0 }))
        .h_full()
        .transition_all()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                crate::ui::motion::FAST_MS
            }
        })
        .bg(move || theme().background)
        .child(
            scroll().flex_1().child(
                view().gap_0p5().p_2().children(
                    DESKTOP_SECTIONS
                        .into_iter()
                        .map(move |(key, glyph, route)| nav_item(nav, key, glyph, route, compact)),
                ),
            ),
        )
        .child(view().p_2().child(nav_item(
            nav,
            "nav.settings.title",
            Icon::Settings,
            Route::Settings,
            compact,
        )))
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
    let selected_route = route.clone();
    let selected = computed(move || grouped::primary(nav.section()) == selected_route);
    let account = (key == "nav.accountFallback")
        .then(use_context::<AccountLabel>)
        .flatten();
    let label = move || {
        account
            .map(|account| account.label.get())
            .unwrap_or_else(|| tr(key))
    };
    crate::ui::controls::surface(ButtonVariant::Quiet)
        .accessibility_label(label)
        .h(px(37.0))
        .min_h(px(37.0))
        .px_3()
        .py_2()
        .flex_row()
        .justify_start()
        .gap_3()
        .text_color(move || theme().muted_foreground)
        .selected(selected)
        .selected_style(crate::ui::palette::selected)
        .child(icon(glyph).text_color(move || {
            if selected.get() {
                crate::ui::palette::selection_foreground()
            } else {
                theme().muted_foreground
            }
        }))
        .child(crate::ui::hint(label, Placement::Right).disabled(move || !compact()))
        .child(when(
            move || !compact(),
            move || {
                text(label)
                    .text_sm()
                    .line_height(px(21.0))
                    .font_weight(FontWeight::NORMAL)
                    .style_when(selected, |style| style.font_weight(FontWeight::MEDIUM))
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
