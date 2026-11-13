use super::{
    SessionHandle,
    model::{Audience, Query, finish_recommendation},
};
use crate::{bridge, i18n::tr, navigation::Navigation, ui};
use hongguo_api::rank::RankPage;
use hongguo_common::model::TaskStatus;
use nsh_core::prelude::*;
use std::future::Future;

pub(super) fn content(session: SessionHandle, nav: Navigation, active: Memo<bool>) -> View {
    let (query, feed) = data(
        active,
        move || session.identity(),
        move |query| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let page = hongguo_api::new_drama::fetch_new_drama(
                    query.audience.value(),
                    query.offset,
                    (!query.session.is_empty()).then_some(query.session.as_str()),
                    &state.api_env(),
                )
                .await?;
                Ok(finish_recommendation(page, query))
            })
        },
    );
    let cards = computed(move || {
        session.downloads();
        let completed = session
            .with(|session| session.state.queue().all())
            .into_iter()
            .filter(|task| task.status == TaskStatus::Completed)
            .fold(
                std::collections::HashMap::<String, usize>::new(),
                |mut counts, task| {
                    *counts.entry(task.series_id).or_default() += 1;
                    counts
                },
            );
        feed.with(|page| {
            page.map(|page| {
                page.items
                    .iter()
                    .cloned()
                    .map(|item| {
                        let downloaded = completed.get(&item.series_id).copied().unwrap_or(0);
                        super::model::card(item, downloaded)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
    });
    let grid = view()
        .flex_1()
        .min_h(px(0.0))
        .child(when(
            move || feed.loading(),
            move || ui::status(move || tr("common.loading")),
        ))
        .child(when(
            move || feed.error().is_some(),
            move || {
                super::super::error_state(
                    move || {
                        feed.error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || feed.refetch(),
                )
            },
        ))
        .child(when(
            move || {
                !feed.loading()
                    && feed.error().is_none()
                    && feed.with(|page| page.is_some_and(|page| page.items.is_empty()))
            },
            move || ui::status(move || tr("newDrama.emptyRecommend")),
        ))
        .child(dynamic(move || {
            query.with(|query| query.audience);
            ui::refresh_shade(move || feed.loading() && cards.with(|cards| !cards.is_empty()))
                .style_when(move || feed.error().is_some(), |style| style.hidden())
                .child(ui::new_drama_grid(
                    cards,
                    move || {
                        feed.with(|page| {
                            page.is_some_and(|page| !page.has_more && !page.items.is_empty())
                        })
                    },
                    move |id| {
                        let item = feed.with(|page| {
                            page.and_then(|page| {
                                page.items.iter().find(|item| item.series_id == id).cloned()
                            })
                        });
                        if let Some(item) = item {
                            nav.detail(crate::navigation::DetailPrefill::from_rank(&item));
                        }
                    },
                    move || {
                        if !active.get_untracked() || feed.loading() || feed.error().is_some() {
                            return;
                        }
                        if let Some(page) = feed.get() {
                            if !page.has_more {
                                return;
                            }
                            query.update(|query| {
                                query.offset = page.next_offset;
                                query.session = page.session_id;
                                query.previous = page.items;
                                query.identity = session.identity();
                            });
                        }
                    },
                    move || query.with(|query| query.audience.value()),
                ))
        }));
    presentation(
        move || query.with(|query| query.audience),
        move |audience| {
            query.set(Query {
                audience,
                identity: session.identity(),
                ..Default::default()
            })
        },
        grid,
    )
}

pub(super) fn data<Fut: Future<Output = Result<RankPage, bridge::Failure>> + 'static>(
    active: Memo<bool>,
    identity: impl Fn() -> u64 + Copy + 'static,
    fetch: impl Fn(Query) -> Fut + 'static,
) -> (Signal<Query>, Resource<RankPage, bridge::Failure>) {
    crate::view_memory::cached("new-recommend", move || {
        let query = crate::view_memory::scope("new").projected(
            "recommend",
            Query {
                identity: identity(),
                ..Default::default()
            },
            |query| Query {
                audience: query.audience,
                ..Default::default()
            },
        );
        crate::view_memory::track_identity(identity, query, |query, identity| {
            query.identity = identity;
            query.offset = 0;
            query.session.clear();
            query.previous.clear();
        });
        let feed = resource(
            move || active.get().then(|| (query.get(), identity())),
            move |(mut query, identity), _| {
                if query.identity != identity {
                    query.offset = 0;
                    query.session.clear();
                    query.previous.clear();
                }
                fetch(query)
            },
        );
        (query, feed)
    })
}

pub(super) fn presentation(
    audience: impl IntoReactive<Audience>,
    select: impl Fn(Audience) + Clone + 'static,
    content: impl IntoElement,
) -> View {
    let audience = audience.into_reactive();
    view()
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_end()
                .flex_wrap()
                .gap_2()
                .children(
                    [
                        (Audience::All, "newDrama.gender.all"),
                        (Audience::Male, "newDrama.gender.male"),
                        (Audience::Female, "newDrama.gender.female"),
                    ]
                    .into_iter()
                    .map(move |(value, label)| {
                        let current = audience.clone();
                        let select = select.clone();
                        ui::history_filter(move || tr(label))
                            .selected(move || current.get() == value)
                            .on_press(move || select(value))
                    }),
                ),
        )
        .child(content)
}
