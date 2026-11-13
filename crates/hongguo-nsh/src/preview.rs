use crate::{
    i18n::tr,
    ui::{self, ButtonVariant},
};
use nsh_base::theme;
use nsh_core::prelude::*;
#[path = "window_preview.rs"]
mod window;

#[component]
/// Offline layout root using the production page presentation components.
pub fn LayoutPreview() -> impl IntoElement {
    crate::view_memory::provide();
    crate::shortcuts::Shortcuts::provide_preview();
    let emoji = signal(false);
    let motion = signal(false);
    let gestures = signal(false);
    let windows = signal(false);
    on_key("mod+shift+e", move |_| emoji.set(true));
    on_key("mod+shift+d", move |_| motion.set(true));
    on_key("mod+shift+g", move |_| gestures.set(true));
    on_key("mod+shift+w", move |_| windows.set(true));
    view()
        .flex_1()
        .child(when(
            move || !motion.get() && !gestures.get() && !windows.get(),
            layout_fixture,
        ))
        .child(when(
            move || motion.get() && !gestures.get() && !windows.get(),
            || {
                crate::player::stage::fixture_with_motion(
                    nsh_core::element::ImageSrc::from(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../target/nsh-layout-fixtures/player-frame.png"
                    )),
                    || {},
                )
            },
        ))
        .child(when(
            move || gestures.get() && !windows.get(),
            crate::player::navigation::preview,
        ))
        .child(when(windows, window::preview))
        .child(ui::sheet(emoji, "表情离线验证", crate::emoji::fixture))
}

fn layout_fixture() -> View {
    let account_dialogs = crate::pages::preview_account_dialogs();
    let nav = crate::navigation::Navigation::new();
    crate::view_memory::provide_models();
    let logged_in = computed(move || {
        matches!(
            nav.section(),
            crate::navigation::Route::Collections
                | crate::navigation::Route::Liked
                | crate::navigation::Route::Follows
                | crate::navigation::Route::Reservations
        )
    });
    ui::provide_account_label(
        computed(move || {
            if logged_in.get() {
                SharedString::from("离线测试账号")
            } else {
                tr("nav.accountFallback")
            }
        }),
        logged_in,
    );
    let selected = signal("16");
    let tabs = view()
        .flex_row()
        .items_center()
        .gap_0p5()
        .pointer_events(nsh_core::PointerEvents::Auto)
        .children(
            [
                ("16", "home.tabFeed"),
                ("36", "home.tabComic"),
                ("39", "collections.filterReal"),
            ]
            .into_iter()
            .map(move |(id, key)| {
                ui::header_tab(move || tr(key))
                    .accessibility_label(move || tr(key))
                    .min_h(px(24.0))
                    .h_6()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .selected(move || selected.get() == id)
                    .selected_style(|style| {
                        style
                            .bg(move || theme().primary)
                            .text_color(move || theme().primary_foreground)
                            .border(px(0.0))
                    })
                    .on_press(move || selected.set(id))
            }),
        )
        .style_when(
            move || {
                nav.routes().with(|routes| {
                    !matches!(
                        routes.last(),
                        Some(
                            crate::navigation::Route::Catalog
                                | crate::navigation::Route::Player(_, _)
                        )
                    )
                })
            },
            |style| style.hidden(),
        );
    let content = ui::stack().child(each(
        nav.routes(),
        |route: &crate::navigation::Route| format!("{route:?}"),
        move |route, index| {
            let screen = match route.get_untracked() {
                crate::navigation::Route::Settings => crate::pages::preview_settings(nav),
                crate::navigation::Route::History => crate::pages::preview_history(
                    nav,
                    nsh_core::element::ImageSrc::from(frame_path()),
                ),
                crate::navigation::Route::Tasks => crate::pages::preview_tasks(nav),
                crate::navigation::Route::Merge => crate::pages::preview_merge(nav),
                crate::navigation::Route::Storage => crate::pages::preview_storage(nav),
                crate::navigation::Route::Creator(id) => crate::pages::preview_creator(nav, id),
                crate::navigation::Route::New => crate::pages::preview_new(nav),
                crate::navigation::Route::Rank => crate::pages::preview_rank(nav),
                crate::navigation::Route::Browse => crate::pages::preview_browse(nav),
                crate::navigation::Route::Collections => {
                    crate::library::preview(nav, crate::library::Kind::Collections)
                }
                crate::navigation::Route::Liked => {
                    crate::library::preview(nav, crate::library::Kind::Liked)
                }
                crate::navigation::Route::Follows => {
                    crate::library::preview(nav, crate::library::Kind::Follows)
                }
                crate::navigation::Route::Reservations => {
                    crate::library::preview(nav, crate::library::Kind::Reservations)
                }
                crate::navigation::Route::Detail(_) => {
                    crate::pages::preview_detail(nav, fixture_series(), fixture_meta())
                }
                crate::navigation::Route::DetailWithPrefill(prefill) => {
                    if prefill.series_id == "7701328280862854168" {
                        crate::pages::preview_detail(nav, fixture_series(), fixture_meta())
                    } else {
                        crate::pages::preview_upcoming(nav, prefill)
                    }
                }
                _ => ui::page(nav, move || tr("nav.home.title")).child(crate::player::fixture(
                    nsh_core::element::ImageSrc::from(frame_path()),
                    move || nav.push(crate::navigation::Route::Detail("fixture".into())),
                )),
            };
            screen.on_dismiss(move || nav.dismiss(index.get_untracked()))
        },
    ));
    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(ui::shell(nav, tabs, content))
        .child(account_dialogs)
}

fn frame_path() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/nsh-layout-fixtures/player-frame.png"
    )
}

fn fixture_series() -> hongguo_common::model::Series {
    hongguo_common::model::Series {
        series_id: "fixture".into(),
        title: "装穷提亲，娶到全家嫌弃的她".into(),
        cover: frame_path().into(),
        episode_count: 80,
        tags: vec!["恋爱".into(), "乡村".into()],
        episodes: (1..=80)
            .map(|index| hongguo_common::model::Episode {
                vid_index: index,
                title: format!("第{index}集"),
                duration: 169,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn fixture_meta() -> hongguo_api::detail::SeriesMeta {
    hongguo_api::detail::SeriesMeta { series_id: "fixture".into(), title: "装穷提亲，娶到全家嫌弃的她".into(), cover: frame_path().into(), followed_cnt: 1_701_000, play_cnt: 15_080_000, hot_score: 41_050_000,
        season: "第1季".into(), tags: vec!["恋爱".into(), "乡村".into()], update_tag: "全80集".into(), intro: "一次平凡的相遇，让两个原本毫无交集的人走到一起。他隐藏身份上门提亲，却意外发现了这个家庭不为人知的故事。".into(), creators: crate::pages::preview_creators(), ..Default::default() }
}

#[component]
/// An isolated surface for reviewing the app's styled controls without network or database access.
pub fn UiPreview() -> impl IntoElement {
    set_tokens(
        nsh_base::tokens_of(Some(include_str!("../theme.json"))).expect("validated NSH theme"),
    );
    let pressed = signal(false);
    let draft = signal(String::new());
    let filters_open = signal(false);
    let autoplay = signal(false);
    let nav = crate::navigation::Navigation::new();
    view()
        .flex_1()
        .p_safe_area()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .flex_row()
        .child(ui::sections(nav))
        .child(ui::form(
            view()
                .p_6()
                .gap_4()
                .child(
                    text(move || tr("app.name"))
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    ui::field(move || tr("nav.browse.title"))
                        .test_id("preview-query")
                        .value(draft)
                        .on_change(move |value: &str| draft.set(value.to_owned())),
                )
                .child(
                    ui::button(move || tr("player.play"), ButtonVariant::Primary)
                        .on_press(move || pressed.update(|value| *value = !*value)),
                )
                .child(
                    ui::button(move || tr("nav.history.title"), ButtonVariant::Secondary)
                        .selected(true),
                )
                .child(ui::button(
                    move || tr("nav.settings.title"),
                    ButtonVariant::Quiet,
                ))
                .child(
                    ui::switch(move || tr("settings.autoNext"))
                        .checked(autoplay)
                        .on_change(move |next: &bool| autoplay.set(*next)),
                )
                .child(
                    ui::button(move || tr("rank.filter.title"), ButtonVariant::Secondary)
                        .on_press(move || filters_open.set(true)),
                )
                .child(ui::video_caption(move || tr("app.name")))
                .child(
                    ui::button(move || tr("common.loading"), ButtonVariant::Secondary)
                        .disabled(true),
                )
                .child(
                    ui::status(move || {
                        if pressed.get() {
                            tr("player.play")
                        } else {
                            tr("common.empty")
                        }
                    })
                    .test_id("preview-status"),
                ),
        ))
        .child(ui::sheet(
            filters_open,
            move || tr("rank.filter.title"),
            move || {
                view()
                    .gap_4()
                    .child(ui::field(move || tr("nav.browse.title")))
                    .child(
                        ui::switch(move || tr("settings.autoNext"))
                            .checked(autoplay)
                            .on_change(move |next: &bool| autoplay.set(*next)),
                    )
            },
        ))
}
