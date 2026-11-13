use super::{
    filters::{Kind, control},
    model::{self, Query},
};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::rank::{RankSubList, RankTab};
use nsh_core::prelude::*;
mod focus_scroll;

pub(super) fn content(
    nav: Navigation,
    query: Signal<Query>,
    source: crate::pages::series_feed::Feed,
    rows: View,
) -> View {
    let tabs = computed(move || {
        source.with(|page| {
            model::normalize(
                page.map(|page| page.tabs.clone()).unwrap_or_default(),
                &tr("rank.allTab"),
                &tr("rank.filter.title"),
            )
        })
    });
    let current = computed(move || {
        tabs.with(|tabs| {
            tabs.iter()
                .find(|tab| tab.id == query.with(|q| q.tab.clone()))
                .and_then(|tab| {
                    tab.subs
                        .iter()
                        .find(|sub| sub.id == query.with(|q| q.sub.clone()))
                        .or_else(|| tab.subs.first())
                })
                .cloned()
        })
    });
    let compact = computed(|| window().width.0 < 768.0);
    effect(move || {
        if source.loading() {
            return;
        }
        let mut next = query.get_untracked();
        let selected = tabs.with(|tabs| {
            tabs.iter()
                .find(|tab| tab.id == next.tab)
                .or_else(|| tabs.first())
                .cloned()
        });
        let Some(tab) = selected else {
            return;
        };
        if tab.id == next.tab && tab.subs.iter().any(|sub| sub.id == next.sub) {
            return;
        }
        next.tab = tab.id;
        next.sub = tab
            .subs
            .first()
            .map(|sub| sub.id.clone())
            .unwrap_or_default();
        next.panel.clear();
        next.background.clear();
        next.reset_paging();
        query.set(next);
    });
    ui::toolbar_slot(
        move || {
            is_desktop()
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::Rank))
        },
        move || content_tabs(tabs, query),
    );
    view()
        .flex_1()
        .min_h(px(0.0))
        .bg(move || nsh_base::theme().background)
        .child(when(
            move || !is_desktop(),
            move || content_tabs(tabs, query),
        ))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .gap_2()
                .style_when(move || !compact.get(), |style| style.flex_row().gap_4())
                .child(dynamic(move || sub_lists(tabs, query, compact.get())))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .gap_2()
                        .child(
                            view()
                                .test_id("rank-header")
                                .h_8()
                                .flex_shrink_0()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    text(move || {
                                        current.get().map(|sub| sub.name).unwrap_or_default()
                                    })
                                    .text_sm()
                                    .font_weight(FontWeight::NORMAL),
                                )
                                .child(
                                    text(move || {
                                        current.get().map(|sub| sub.description).unwrap_or_default()
                                    })
                                    .text_xs()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(move || nsh_base::theme().muted_foreground)
                                    .flex_1()
                                    .number_of_lines(1),
                                )
                                .child(control(current, query, Kind::Month))
                                .child(control(current, query, Kind::Total)),
                        )
                        .child(rows),
                ),
        )
}

fn sub_lists(tabs: Memo<Vec<RankTab>>, query: Signal<Query>, compact: bool) -> Scroll {
    let scroll_state = focus_scroll::FocusScroll::new(compact);
    let entries = computed(move || {
        tabs.with(|tabs| {
            tabs.iter()
                .find(|tab| tab.id == query.with(|q| q.tab.clone()))
                .map(|tab| tab.subs.clone())
                .unwrap_or_default()
        })
    });
    effect(move || {
        scroll_state.retain(&entries.get());
    });
    let mut viewport = scroll()
        .key(if compact {
            "sub-lists-horizontal"
        } else {
            "sub-lists-vertical"
        })
        .test_id("rank-sub-lists")
        .element_ref(&scroll_state.node)
        .on_layout(move |bounds: &Bounds<Points>| scroll_state.layout(bounds))
        .on_scroll(move |event: &nsh_core::element::ScrollEvent| scroll_state.moved(event))
        .label(move || tr("rank.subLists"))
        .min_h_0()
        .flex_shrink_0();
    let mut content = ui::rank_sub_group();
    if compact {
        viewport = viewport
            .horizontal()
            .w_full()
            .h(px(if is_desktop() { 40.0 } else { 52.0 }))
            .pb_1();
        content = content.flex_row();
    } else {
        viewport = viewport.w(px(144.0)).h_full();
        content = content.w_full();
    }
    viewport.child(content.child(each(
        entries,
        |sub: &RankSubList| sub.id.clone(),
        move |sub, _| {
            let reference = element_ref();
            reference.on_focus_in(move || scroll_state.focus(sub.get_untracked().id));
            view()
                .element_ref(&reference)
                .flex_shrink_0()
                .style_when(move || !compact, |style| style.w_full())
                .on_layout(move |bounds: &Bounds<Points>| {
                    scroll_state.row(&sub.get_untracked().id, bounds)
                })
                .child(
                    ui::rank_sub_tab(
                        move || SharedString::from(sub.get().name),
                        move || query.with(|q| q.sub == sub.with(|sub| sub.id.clone())),
                    )
                    .on_change(move |pressed: &bool| {
                        if *pressed {
                            query.update(|q| {
                                q.sub = sub.get_untracked().id;
                                q.panel.clear();
                                q.background.clear();
                                q.reset_paging();
                            });
                        }
                    }),
                )
        },
    )))
}

fn content_tabs(tabs: Memo<Vec<RankTab>>, query: Signal<Query>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_0p5()
        .pointer_events(nsh_core::PointerEvents::Auto)
        .child(each(
            move || tabs.get(),
            |tab: &RankTab| tab.id.clone(),
            move |tab, _| {
                ui::header_tab(move || SharedString::from(tab.get().name))
                    .selected(move || query.with(|q| q.tab == tab.with(|tab| tab.id.clone())))
                    .on_press(move || {
                        let tab = tab.get_untracked();
                        query.update(|q| {
                            q.tab = tab.id;
                            q.sub = tab
                                .subs
                                .first()
                                .map(|sub| sub.id.clone())
                                .unwrap_or_default();
                            q.panel.clear();
                            q.background.clear();
                            q.reset_paging();
                        });
                    })
            },
        ))
}
