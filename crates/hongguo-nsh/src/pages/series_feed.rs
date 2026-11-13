use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::rank::{RankItem, RankPage};
use nsh_core::prelude::*;
use std::collections::{HashMap, HashSet};

pub(super) type Feed = Resource<RankPage, bridge::Failure>;

pub(super) fn from_feed(page: hongguo_api::discover::FeedPage) -> RankPage {
    RankPage {
        items: page
            .items
            .into_iter()
            .map(|item| RankItem {
                series_id: item.series_id,
                title: item.title,
                cover: item.cover,
                vid: item.vid,
                episode_cnt: item.episode_cnt,
                score: item.score,
                play_cnt: item.play_cnt,
                tags: item.tags,
                rec_text: item.heat_text,
                season: item.season_tag,
                ..Default::default()
            })
            .collect(),
        has_more: page.has_more,
        next_offset: page.next_offset,
        session_id: page.session_id,
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Reservation {
    pub(super) changed: Signal<HashMap<String, bool>>,
    pub(super) pending: Signal<HashSet<String>>,
    pub(super) feedback: Signal<SharedString>,
}

pub(super) fn reservation(
    session: SessionHandle,
    nav: Navigation,
) -> (Reservation, impl Fn(RankItem) + Copy + 'static) {
    let changed = signal(HashMap::<String, bool>::new());
    let pending = signal(HashSet::<String>::new());
    let feedback = signal(SharedString::default());
    let identity = stored(session.identity());
    effect(move || {
        let next = session.identity();
        if identity.replace(next) != next {
            changed.set(HashMap::new());
            pending.set(HashSet::new());
            feedback.set(SharedString::default());
        }
    });
    let toggle = move |item: RankItem| {
        let state = session.with(|session| session.state.clone());
        if state.settings().account.is_none() {
            nav.push(Route::Account);
            return;
        }
        if pending.with(|pending| pending.contains(&item.series_id)) {
            return;
        }
        let identity = session.identity();
        let before = changed
            .with(|changed| changed.get(&item.series_id).copied())
            .unwrap_or(item.reserved);
        changed.update(|changed| {
            changed.insert(item.series_id.clone(), !before);
        });
        pending.update(|pending| {
            pending.insert(item.series_id.clone());
        });
        feedback.set(SharedString::default());
        spawn(async move {
            let id = item.series_id.clone();
            let outcome = bridge::request(async move {
                hongguo_api::reservation::reserve_series(&id, !before, &state.api_env()).await
            })
            .await;
            if session.identity() != identity {
                return;
            }
            match outcome {
                Ok(()) => {
                    session.notify_library();
                }
                Err(error) => {
                    changed.update(|changed| {
                        changed.insert(item.series_id.clone(), before);
                    });
                    feedback.set(error.text());
                }
            }
            pending.update(|pending| {
                pending.remove(&item.series_id);
            });
        });
    };
    (
        Reservation {
            changed,
            pending,
            feedback,
        },
        toggle,
    )
}

#[derive(Clone, Copy)]
pub(super) struct Paging {
    pub loading: Memo<bool>,
    pub request: Stored<Box<dyn Fn()>>,
}

#[derive(Clone, Copy)]
struct RankActions {
    reservation: Reservation,
    reserve: Stored<Box<dyn Fn(RankItem)>>,
    play: Option<crate::player::StartAction>,
    paging: Paging,
}

pub(super) fn rank_list(
    feed: Feed,
    session: SessionHandle,
    nav: Navigation,
    paging: Paging,
    scope: Memo<super::rank::ScrollScope>,
) -> View {
    let (reservation, toggle) = reservation(session, nav);
    rank_rows(
        feed,
        nav,
        RankActions {
            reservation,
            reserve: stored(Box::new(toggle) as Box<dyn Fn(RankItem)>),
            play: Some(crate::player::start_action(session, nav)),
            paging,
        },
        scope,
    )
}

pub(super) fn preview_rank_list(
    feed: Feed,
    nav: Navigation,
    scope: Memo<super::rank::ScrollScope>,
) -> View {
    let reservation = Reservation {
        changed: signal(HashMap::new()),
        pending: signal(HashSet::new()),
        feedback: signal(SharedString::default()),
    };
    let toggle = move |item: RankItem| {
        let selected = reservation
            .changed
            .with(|changed| changed.get(&item.series_id).copied())
            .unwrap_or(item.reserved);
        reservation.changed.update(|changed| {
            changed.insert(item.series_id, !selected);
        });
    };
    rank_rows(
        feed,
        nav,
        RankActions {
            reservation,
            reserve: stored(Box::new(toggle) as Box<dyn Fn(RankItem)>),
            play: None,
            paging: Paging {
                loading: computed(|| false),
                request: stored(Box::new(|| {}) as Box<dyn Fn()>),
            },
        },
        scope,
    )
}

fn rank_metadata(item: &RankItem) -> String {
    let score = (item.score > 0.0).then(|| format!("{:.1}", item.score));
    std::iter::once(item.sub_title.as_str())
        .chain(score.as_deref())
        .chain(item.tags.iter().map(String::as_str))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn rank_rows(
    feed: Feed,
    nav: Navigation,
    actions: RankActions,
    scope: Memo<super::rank::ScrollScope>,
) -> View {
    let RankActions {
        reservation,
        reserve,
        play,
        paging,
    } = actions;
    let initial = computed(move || {
        feed.loading() && feed.with(|page| page.is_none_or(|page| page.items.is_empty()))
    });
    view()
        .flex_1()
        .min_h(px(0.0))
        .gap_2()
        .child(when(initial, ui::rank_loading))
        .child(when(
            move || feed.error().is_some(),
            move || {
                super::error_state(
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
                !feed.loading() && feed.with(|page| page.is_some_and(|page| page.items.is_empty()))
            },
            move || ui::status(move || tr("rank.empty")),
        ))
        .child(ui::status(reservation.feedback))
        .child(when(
            move || play.is_some_and(|play| !play.notice().is_empty()),
            move || ui::status(move || play.map(|play| play.notice()).unwrap_or_default()),
        ))
        .child(
            ui::refresh_shade(move || feed.loading() && !initial.get())
                .style_when(initial, |style| style.hidden())
                .child(crate::view_memory::remember_scroll(
                    virtual_list(
                        move || feed.with(|page| page.map_or(0, |page| page.items.len())),
                        move |at| {
                            let item = move || {
                                feed.with(|page| {
                                    page.and_then(|page| page.items.get(at.index())).cloned()
                                })
                            };
                            ui::rank_row(
                                move || {
                                    item()
                                        .map(|item| {
                                            let reserved = reservation
                                                .changed
                                                .with(|changed| {
                                                    changed.get(&item.series_id).copied()
                                                })
                                                .unwrap_or(item.reserved);
                                            let pending = reservation
                                                .pending
                                                .with(|pending| pending.contains(&item.series_id))
                                                || play.is_some_and(|play| {
                                                    play.pending(&item.series_id)
                                                });
                                            let metadata = rank_metadata(&item);
                                            let secondary = item
                                                .secondary_infos
                                                .iter()
                                                .filter(|info| {
                                                    !info.is_empty() && **info != item.rec_text
                                                })
                                                .cloned()
                                                .collect::<Vec<_>>()
                                                .join("  ");
                                            ui::RankRowData {
                                                title: item.title,
                                                cover: item.cover,
                                                rank: item.rank,
                                                season: item.season,
                                                metadata,
                                                description: item.description,
                                                heat: item.rec_text,
                                                secondary,
                                                upcoming: item.upcoming,
                                                reserved,
                                                pending,
                                            }
                                        })
                                        .unwrap_or_default()
                                },
                                move || {
                                    if let Some(item) = item() {
                                        nav.detail(crate::navigation::DetailPrefill::from_rank(
                                            &item,
                                        ));
                                    }
                                },
                                move || {
                                    if let Some(item) = item() {
                                        if let Some(play) = play {
                                            play.start(item.series_id);
                                        } else {
                                            nav.push(Route::Player(item.series_id, 1));
                                        }
                                    }
                                },
                                move || {
                                    if let Some(item) = item() {
                                        reserve.with(|reserve| reserve(item));
                                    }
                                },
                            )
                        },
                    )
                    .test_id("rank-results")
                    .flex_1()
                    .on_load_more(move || paging.request.with(|request| request()))
                    .footer(view().child(when(paging.loading, ui::browse_loading_more)))
                    .row_gap(px(8.0))
                    .estimated_item_size(px(133.0))
                    .keys(move || {
                        feed.with(|page| {
                            page.map(|page| {
                                page.items
                                    .iter()
                                    .map(|item| item.series_id.clone())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                        })
                    }),
                    "rank-results-scroll",
                    scope,
                )),
        )
}

pub(super) fn append(mut previous: Vec<RankItem>, incoming: Vec<RankItem>) -> Vec<RankItem> {
    let mut positions: HashMap<String, usize> = previous
        .iter()
        .enumerate()
        .map(|(index, item)| (item.series_id.clone(), index))
        .collect();
    for item in incoming {
        if let Some(index) = positions.get(&item.series_id) {
            previous[*index] = item;
        } else {
            positions.insert(item.series_id.clone(), previous.len());
            previous.push(item);
        }
    }
    for (index, item) in previous.iter_mut().enumerate() {
        if item.rank > 0 {
            item.rank = index as u32 + 1;
        }
    }
    previous
}

#[cfg(test)]
mod tests {
    #[test]
    fn rank_metadata_matches_subtitle_score_and_tags_without_empty_separators() {
        let item = super::RankItem {
            sub_title: "恋爱 · 全80集".into(),
            score: 9.2,
            tags: vec!["恋爱".into(), "乡村".into()],
            ..Default::default()
        };
        assert_eq!(
            super::rank_metadata(&item),
            "恋爱 · 全80集 · 9.2 · 恋爱 · 乡村"
        );
        assert_eq!(super::rank_metadata(&super::RankItem::default()), "");
    }
    use super::*;

    fn item(id: &str, title: &str, rank: u32) -> RankItem {
        RankItem {
            series_id: id.into(),
            title: title.into(),
            rank,
            ..Default::default()
        }
    }

    #[test]
    fn pagination_updates_duplicates_without_moving_rows_and_renumbers_ranked_items() {
        let result = append(
            vec![item("a", "Old", 1), item("b", "Second", 2)],
            vec![
                item("b", "Updated", 1),
                item("c", "Third", 2),
                item("c", "Latest", 2),
            ],
        );
        assert_eq!(
            result
                .iter()
                .map(|item| (item.series_id.as_str(), item.title.as_str(), item.rank))
                .collect::<Vec<_>>(),
            vec![("a", "Old", 1), ("b", "Updated", 2), ("c", "Latest", 3)]
        );
    }

    #[test]
    fn reservation_rows_never_gain_an_artificial_rank() {
        let result = append(vec![item("a", "First", 0)], vec![item("b", "Second", 0)]);
        assert!(result.iter().all(|item| item.rank == 0));
    }
}
