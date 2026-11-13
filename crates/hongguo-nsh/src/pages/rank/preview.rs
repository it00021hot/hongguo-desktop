//! Fixed, offline inputs for the production rank presenter. The stress shortcut
//! changes only this preview's data; normal screenshots use the shared fixture.
use super::*;
use hongguo_api::rank::{RankPage, RankPanelItem, RankPanelRow};

pub(super) fn build(nav: Navigation) -> Screen {
    let query = crate::view_memory::scope("rank").projected(
        "query",
        Query::default(),
        Query::view_snapshot,
    );
    let page = stored(super::super::fixtures::catalog().rank);
    let stress = signal(false);
    let sub_lists_stress = signal(false);
    effect(move || {
        if nav
            .routes()
            .with(|routes| routes.last() == Some(&Route::Rank))
        {
            on_key("mod+shift+r", move |_| {
                stress.update(|value| *value = !*value)
            });
            on_key("mod+shift+u", move |_| {
                sub_lists_stress.update(|value| *value = !*value)
            });
        }
    });
    let source: series_feed::Feed = resource(
        move || {
            let page = if stress.get() {
                long_filters(page.get())
            } else {
                page.get()
            };
            Some(if sub_lists_stress.get() {
                long_sub_lists(page)
            } else {
                page
            })
        },
        |page, _| std::future::ready(Ok(page)),
    );
    ui::page(nav, move || tr("nav.rank.title")).child(presentation::content(
        nav,
        query,
        source,
        series_feed::preview_rank_list(
            source,
            nav,
            computed(move || query.with(Query::scroll_scope)),
        ),
    ))
}

fn long_sub_lists(mut page: RankPage) -> RankPage {
    const SUB_LIST_COUNT: usize = 24;
    for tab in &mut page.tabs {
        let Some(template) = tab.subs.first() else {
            continue;
        };
        tab.subs = (1..=SUB_LIST_COUNT)
            .map(|index| hongguo_api::rank::RankSubList {
                id: format!("preview-sub-{index}"),
                name: format!("离线子榜 {index:02}"),
                ..template.clone()
            })
            .collect();
    }
    page
}

fn long_filters(mut page: RankPage) -> RankPage {
    for tab in &mut page.tabs {
        for sub in &mut tab.subs {
            sub.panel = (1..=18)
                .map(|row| RankPanelRow {
                    name: format!("离线题材组 {row:02}"),
                    items: (1..=8)
                        .map(|column| RankPanelItem {
                            id: format!("stress-{row}-{column}"),
                            name: if row == 18 && column == 8 {
                                "长列表末项".into()
                            } else {
                                format!("选项 {row:02}-{column:02}")
                            },
                        })
                        .collect(),
                })
                .collect();
            sub.background = vec![RankPanelRow {
                name: "离线月份".into(),
                items: (2018..=2026)
                    .flat_map(|year| {
                        (1..=12).map(move |month| RankPanelItem {
                            id: format!("stress-{year}-{month}"),
                            name: format!("{year}年{month}月"),
                        })
                    })
                    .collect(),
            }];
        }
    }
    page
}
