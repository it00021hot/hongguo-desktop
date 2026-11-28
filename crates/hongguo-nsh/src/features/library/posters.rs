use super::{Kind, model::Entry, rows::Rows};
use crate::{i18n::tr, navigation::Route};
use nsh_core::prelude::*;

pub(super) fn build(
    rows: Rows,
    remove: impl Fn(Vec<Entry>) + Copy + 'static,
    play: impl Fn(Entry) + Copy + 'static,
    more: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let width = crate::view_memory::scope(rows.kind.memory_name()).signal("grid-width", 0.0_f32);
    let columns = computed(move || {
        if rows
            .scroll_scope
            .with(|scope| matches!(scope.4, super::CollectionFolder::Official(_)))
        {
            let available = width.get();
            return if available > 0.0 {
                ((available + 16.0) / 182.0).ceil().clamp(2.0, 12.0) as u32
            } else {
                crate::features::library::presentation::personal_poster_columns(window().width.0)
            };
        }
        crate::features::library::presentation::personal_poster_columns(window().width.0)
    });
    let liked = rows.kind == Kind::Liked;
    let card_height = computed(move || {
        let columns = columns.get() as f32;
        let card = (width.get() - (columns - 1.0) * 16.0) / columns;
        (card - 2.0).max(0.0) * 4.0 / 3.0 + 62.0
    });
    view()
        .flex_1()
        .min_h(px(0.0))
        .max_h(move || {
            let count = rows.items.with(Vec::len).div_ceil(columns.get() as usize);
            px(count as f32 * card_height.get() + count.saturating_sub(1) as f32 * 16.0)
        })
        .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .child(
            crate::view_memory::remember_scroll(
                virtual_list(
                    move || rows.items.with(Vec::len),
                    move |at| {
                        let item = computed(move || {
                            rows.items.with(|items| items.get(at.index()).cloned()).map(
                                |mut item| {
                                    if !liked {
                                        rows.cards.hydrate(&mut item);
                                    }
                                    item
                                },
                            )
                        });
                        let picked = move || {
                            item.get().is_some_and(|item| {
                                rows.selected.with(|keys| keys.contains(&item.key))
                            })
                        };
                        view()
                            .style_when(move || item.get().is_none(), |style| style.hidden())
                            .relative()
                            .group()
                            .child(
                                crate::features::library::presentation::personal_poster(
                                    move || item.get().map(|item| title(&item)).unwrap_or_default(),
                                    move || item.get().map(|item| item.cover).unwrap_or_default(),
                                    move || {
                                        item.get()
                                            .map(|item| summary(&item, liked, rows.cards))
                                            .unwrap_or_default()
                                    },
                                    liked,
                                    move || !rows.managing.get(),
                                )
                                .disabled(move || {
                                    rows.busy.get()
                                        || rows.managing.get()
                                        || item.get().is_none_or(|item| !item.can_play)
                                })
                                .disabled_style(|style| {
                                    style.opacity(
                                        move || if rows.managing.get() { 1.0_f32 } else { 0.5 },
                                    )
                                })
                                .on_press(move || {
                                    if rows.managing.get_untracked() {
                                        return;
                                    }
                                    if let Some(item) =
                                        item.get_untracked().filter(|item| item.can_play)
                                    {
                                        if liked {
                                            play(item);
                                        } else {
                                            rows.nav.push(Route::Detail(item.series_id));
                                        }
                                    }
                                }),
                            )
                            .child(when(rows.managing, move || {
                                crate::features::library::presentation::personal_card_pick(
                                    move || item.get().map(|item| title(&item)).unwrap_or_default(),
                                    picked,
                                )
                                .disabled(rows.busy)
                                .on_change(
                                    move |checked: &bool| {
                                        if let Some(item) = item.get_untracked() {
                                            rows.selected.update(|keys| {
                                                keys.retain(|key| key != &item.key);
                                                if *checked {
                                                    keys.push(item.key);
                                                }
                                            });
                                        }
                                    },
                                )
                            }))
                            .child(when(
                                move || !rows.managing.get(),
                                move || {
                                    crate::features::library::presentation::personal_poster_remove(
                                        move || tr(rows.kind.action()),
                                        liked,
                                        move || {
                                            !liked
                                                && item.get().is_some_and(|item| {
                                                    !item.can_play
                                                        && rows.cards.status(&item).is_some()
                                                })
                                        },
                                    )
                                    .disabled(rows.busy)
                                    .on_press(move || {
                                        if let Some(item) = item.get_untracked() {
                                            remove(vec![item]);
                                        }
                                    })
                                },
                            ))
                            .into_element()
                    },
                ),
                rows.kind.memory_name(),
                rows.scroll_scope,
            )
            .flex_1()
            .on_load_more(move || {
                if rows.has_more.get_untracked() {
                    more();
                }
            })
            .minimum_view_time(super::metadata::VIEW_SETTLE_TIME)
            .on_viewable_items_change(move |viewable: &nsh_core::element::ViewableItems| {
                if !liked {
                    rows.cards.report(viewable);
                }
            })
            .columns(columns)
            .column_gap(px(16.0))
            .row_gap(px(16.0))
            .keys(move || {
                rows.items.with(|items| {
                    items
                        .iter()
                        .map(|item| item.key.clone())
                        .collect::<Vec<_>>()
                })
            })
            .estimated_item_size(move || px(card_height.get())),
        )
}

pub(super) fn title(entry: &Entry) -> SharedString {
    if entry.title.trim().is_empty() {
        tr("collections.unresolved")
            .replace(
                "{id}",
                &entry
                    .series_id
                    .chars()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>(),
            )
            .into()
    } else {
        entry.title.clone().into()
    }
}

fn summary(entry: &Entry, liked: bool, cards: super::metadata::Cards) -> SharedString {
    if liked {
        if entry.digged_count > 0 {
            entry.digged_count.to_string().into()
        } else {
            SharedString::default()
        }
    } else if !entry.can_play {
        match cards.status(entry) {
            None => tr("collections.metadataLoading"),
            Some(hongguo_service::library_service::CollectionMetadata::Failed) => {
                tr("collections.metadataFailed")
            }
            _ => tr("collections.unavailableHint"),
        }
    } else {
        tr("collections.episodeCount")
            .replace("{n}", &entry.episode_count.to_string())
            .into()
    }
}
