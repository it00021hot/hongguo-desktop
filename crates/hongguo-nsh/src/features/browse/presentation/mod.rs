use super::{
    Feed,
    filters::{self, Panel},
    model::Query,
    search::{self, Search},
};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;
use std::collections::HashMap;

pub(super) fn content(
    nav: Navigation,
    search: Search,
    query: Signal<Query>,
    panel: Panel,
    feed: Feed,
    downloaded: Memo<HashMap<String, usize>>,
    load_more: impl Fn() + 'static,
) -> View {
    let expanded = super::preferences::expanded();
    let load_more = stored(Box::new(load_more) as Box<dyn Fn()>);
    let submit = move || {
        search.dismiss();
        let term = search.term();
        if !term.is_empty() {
            if !is_desktop() {
                dismiss_keyboard();
            }
            query.update(|q| {
                q.term = term;
                q.reset_paging();
            });
        }
    };
    let cards = computed(move || {
        feed.with(|page| {
            page.filter(|loaded| loaded.scope == query.with(Query::scope))
                .map(|loaded| {
                    loaded
                        .page
                        .items
                        .iter()
                        .map(|item| crate::features::browse::presentation::BrowseCard {
                            id: item.series_id.clone(),
                            title: item.title.clone(),
                            cover: item.cover.clone(),
                            heat: if query.with(|query| !query.term.is_empty())
                                && item.rec_text.is_empty()
                                && item.play_cnt > 0
                            {
                                tr("browse.playCount").replace(
                                    "{count}",
                                    &crate::i18n::format_count_precise(item.play_cnt),
                                )
                            } else {
                                item.rec_text.clone()
                            },
                            tags: if query.with(|query| !query.term.is_empty())
                                && item.tags.is_empty()
                            {
                                item.sub_title
                                    .split('·')
                                    .take(1)
                                    .filter(|part| !part.is_empty())
                                    .map(str::to_owned)
                                    .collect()
                            } else {
                                item.tags.iter().take(3).cloned().collect()
                            },
                            episodes: item.episode_cnt,
                            downloaded: downloaded.with(|counts| {
                                counts.get(&item.series_id).copied().unwrap_or_default()
                            }),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    });
    let pending = computed(move || feed.loading() && cards.with(Vec::is_empty));
    let fetching_more = computed(move || feed.loading() && query.with(|query| query.offset > 0));
    let resubmitting = computed(move || {
        feed.loading() && query.with(|query| !query.term.is_empty() && query.term == search.term())
    });
    effect(move || {
        if query.with(|query| !query.term.is_empty())
            && !feed.loading()
            && feed.error().is_none()
            && cards.with(|cards| !cards.is_empty() && cards.len() < 18)
        {
            load_more.with(|run| run());
        }
    });
    let controls = move || {
        view()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                view()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .child(search::field(search, submit, move |item| {
                        if item.cover.is_empty() || item.series_id.is_empty() {
                            search.change(&item.word);
                            submit();
                        } else {
                            if !is_desktop() {
                                dismiss_keyboard();
                            }
                            nav.push(Route::Detail(item.series_id));
                        }
                    }))
                    .child(
                        crate::features::browse::presentation::browse_search_action(resubmitting)
                            .absolute()
                            .left_0()
                            .top_0()
                            .disabled(move || search.term().is_empty() || resubmitting.get())
                            .on_press(submit),
                    ),
            )
            .child(when(
                move || query.with(|q| !q.term.is_empty()),
                move || {
                    crate::features::browse::presentation::browse_exit_action().on_press(
                        move || {
                            search.clear();
                            if !is_desktop() {
                                dismiss_keyboard();
                            }
                            query.update(|q| {
                                q.term.clear();
                                q.reset_paging();
                            });
                        },
                    )
                },
            ))
    };
    view()
        .flex_1()
        .min_h_0()
        .child(when(
            move || !is_desktop(),
            move || view().px_4().py_2().flex_shrink_0().child(controls()),
        ))
        .child(ui::form_with_scroll(
            view()
                .p_4()
                .gap_4()
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(when(
                            move || is_desktop() && query.with(|query| !query.term.is_empty()),
                            move || {
                                crate::features::browse::presentation::browse_exit_action()
                                    .on_press(move || {
                                        search.clear();
                                        query.update(|query| {
                                            query.term.clear();
                                            query.reset_paging();
                                        });
                                    })
                            },
                        ))
                        .child(when(
                            move || query.with(|q| q.term.is_empty()),
                            move || {
                                crate::features::browse::presentation::browse_expander(
                                    expanded,
                                    computed(move || query.with(Query::selected_filter_count)),
                                    move || super::preferences::toggle(expanded),
                                )
                            },
                        ))
                        .child(when(
                            move || {
                                query.with(|query| query.term.is_empty())
                                    && !cards.with(Vec::is_empty)
                            },
                            move || {
                                text(move || {
                                    tr("browse.totalCount")
                                        .replace("{total}", &cards.with(Vec::len).to_string())
                                })
                                .text_sm()
                                .ml(auto())
                                .font_weight(FontWeight::NORMAL)
                                .text_color(move || nsh_base::theme().muted_foreground)
                            },
                        )),
                )
                .child(when(
                    move || query.with(|q| q.term.is_empty()),
                    move || {
                        view().gap_1p5().child(when(
                            move || expanded.get(),
                            move || filters::content(panel, query),
                        ))
                    },
                ))
                .child(when(
                    move || pending.get(),
                    crate::features::browse::presentation::browse_loading,
                ))
                .child(when(
                    move || feed.error().is_some(),
                    move || {
                        crate::features::error_state(
                            move || {
                                feed.error()
                                    .map_or_else(SharedString::default, |error| error.text())
                            },
                            move || feed.refetch(),
                        )
                    },
                ))
                .child(when(
                    move || !pending.get() && cards.with(Vec::is_empty),
                    move || {
                        crate::features::browse::presentation::browse_empty(move || {
                            tr(if query.with(|query| query.term.is_empty()) {
                                "browse.empty"
                            } else {
                                "search.empty"
                            })
                        })
                    },
                ))
                .child(when(
                    move || !cards.with(Vec::is_empty),
                    move || {
                        ui::refresh_shade(move || feed.loading() && !fetching_more.get()).child(
                            crate::features::browse::presentation::browse_grid(
                                cards,
                                move |id| {
                                    if let Some(seed) = feed.with(|page| {
                                        page.and_then(|page| {
                                            page.page.items.iter().find(|item| item.series_id == id)
                                        })
                                        .map(crate::navigation::DetailPrefill::from_rank)
                                    }) {
                                        nav.detail(seed);
                                    }
                                },
                                move || load_more.with(|run| run()),
                            ),
                        )
                    },
                ))
                .child(when(
                    move || query.with(|query| query.term.is_empty()) && fetching_more.get(),
                    crate::features::browse::presentation::browse_loading_more,
                ))
                .child(when(
                    move || {
                        query.with(|query| query.term.is_empty())
                            && !feed.loading()
                            && feed.with(|page| {
                                page.is_some_and(|loaded| {
                                    !loaded.page.has_more && !cards.with(Vec::is_empty)
                                })
                            })
                    },
                    move || {
                        text(move || tr("feed.end"))
                            .text_center()
                            .py_2()
                            .text_sm()
                            .text_color(move || nsh_base::theme().muted_foreground)
                    },
                )),
            crate::view_memory::remember_scroll(
                scroll(),
                "browse.scroll",
                computed(move || query.with(|query| (query.scope(), query.identity))),
            ),
        ))
}

pub(crate) mod browse;
pub(crate) use browse::{
    BrowseCard, browse_chip, browse_exit_action, browse_expander, browse_filter_group, browse_grid,
    browse_search_action,
};

pub(crate) mod browse_loading;
pub(crate) use browse_loading::{
    browse_empty, browse_loading, browse_loading_more, browse_panel_loading,
};

pub(crate) mod search_suggestions;
pub(crate) use search_suggestions::{
    SearchPopup, SearchSuggestion, search_field, search_suggestions,
};
