use super::{Kind, model::Entry, rows::Rows};
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) fn build(
    rows: Rows,
    remove: impl Fn(Vec<Entry>) + Copy + 'static,
    play: impl Fn(Entry) + Copy + 'static,
) -> impl IntoElement {
    let width = crate::view_memory::scope(rows.kind.memory_name()).signal("grid-width", 0.0_f32);
    let columns = computed(move || ui::personal_poster_columns(window().width.0));
    let liked = rows.kind == Kind::Liked;
    let card_height = computed(move || {
        let columns = columns.get() as f32;
        let card = (width.get() - (columns - 1.0) * 12.0) / columns;
        (card - 2.0).max(0.0) * 4.0 / 3.0 + 62.0
    });
    view()
        .flex_1()
        .min_h(px(0.0))
        .max_h(move || {
            let count = rows.items.with(Vec::len).div_ceil(columns.get() as usize);
            px(count as f32 * card_height.get() + count.saturating_sub(1) as f32 * 12.0)
        })
        .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .child(
            crate::view_memory::remember_scroll(
                virtual_list(
                    move || rows.items.with(Vec::len),
                    move |at| {
                        let item = computed(move || {
                            rows.items.with(|items| items.get(at.index()).cloned())
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
                                ui::personal_poster(
                                    move || item.get().map(|item| title(&item)).unwrap_or_default(),
                                    move || item.get().map(|item| item.cover).unwrap_or_default(),
                                    move || {
                                        item.get()
                                            .map(|item| summary(&item, liked))
                                            .unwrap_or_default()
                                    },
                                    liked,
                                )
                                .selected(picked)
                                .disabled(move || {
                                    rows.busy.get()
                                        || (!rows.managing.get()
                                            && item.get().is_none_or(|item| !item.can_play))
                                })
                                .on_press(move || {
                                    let Some(item) = item.get_untracked() else {
                                        return;
                                    };
                                    if rows.managing.get_untracked() {
                                        rows.selected.update(|selected| {
                                            if selected.contains(&item.key) {
                                                selected.retain(|key| key != &item.key);
                                            } else {
                                                selected.push(item.key);
                                            }
                                        });
                                    } else {
                                        play(item);
                                    }
                                }),
                            )
                            .child(when(rows.managing, move || {
                                ui::personal_poster_pick(picked)
                            }))
                            .child(when(
                                move || !rows.managing.get(),
                                move || {
                                    ui::personal_poster_remove(
                                        move || tr(rows.kind.action()),
                                        liked,
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
            .test_id("personal-posters")
            .flex_1()
            .columns(columns)
            .column_gap(px(12.0))
            .row_gap(px(12.0))
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

fn title(entry: &Entry) -> SharedString {
    if entry.title.is_empty() {
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

fn summary(entry: &Entry, liked: bool) -> SharedString {
    if liked {
        if entry.digged_count > 0 {
            entry.digged_count.to_string().into()
        } else {
            SharedString::default()
        }
    } else if entry.episode_count > 0 {
        tr("collections.episodeCount")
            .replace("{n}", &entry.episode_count.to_string())
            .into()
    } else {
        tr("collections.resolvingHint")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;
    use std::time::Duration;

    #[derive(Clone, Copy)]
    struct ScrollFilter(Signal<i64>);

    #[component]
    fn RememberedPosters() -> impl IntoElement {
        crate::view_memory::provide();
        let shown = signal(true);
        let filter = signal(0_i64);
        provide_context(ScrollFilter(filter));
        view()
            .flex_1()
            .child(
                ui::button("Leave posters", ui::ButtonVariant::Quiet)
                    .on_press(move || shown.set(false)),
            )
            .child(
                ui::button("Return posters", ui::ButtonVariant::Quiet)
                    .on_press(move || shown.set(true)),
            )
            .child(
                ui::button("Filter posters", ui::ButtonVariant::Quiet)
                    .on_press(move || filter.set(1)),
            )
            .child(when(shown, || RecycledPosters::element(())))
    }

    #[component]
    fn RecycledPosters() -> impl IntoElement {
        let filter = use_context::<ScrollFilter>();
        let opened = signal(SharedString::default());
        let items = computed(move || {
            (0..100)
                .map(|index| Entry {
                    key: index.to_string(),
                    series_id: index.to_string(),
                    title: format!("Poster {index}"),
                    can_play: true,
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        });
        let rows = Rows {
            kind: Kind::Collections,
            nav: crate::navigation::Navigation::new(),
            items,
            has_more: computed(|| false),
            scroll_scope: computed(move || {
                (
                    0,
                    true,
                    String::new(),
                    filter.map_or(0, |filter| filter.0.get()),
                )
            }),
            busy: signal(false),
            managing: signal(false),
            selected: signal(Vec::new()),
        };
        view()
            .flex_1()
            .child(build(
                rows,
                move |_| panic!("opening a poster must not remove it"),
                move |entry| opened.set(format!("opened:{}", entry.key).into()),
            ))
            .child(text(opened))
    }

    #[test]
    fn recycled_posters_play_the_current_entry() -> Result<(), String> {
        let mut config = EngineConfig::new(
            Root::of::<RecycledPosters>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json")))?),
        );
        config.environment.os = nsh_core::platform::Os::MacOs;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.set_viewport(size(px(800.0), px(360.0)))?;
        app.wait_for("Poster 0")?;
        app.scroll("id:personal-posters", px(1664.0))?;
        app.wait_for("Poster 12")?;
        app.press("role:button[Poster 12]")?;
        app.wait_for("text:opened:12")?;
        Ok(())
    }

    #[test]
    fn poster_scroll_survives_page_remount_and_resets_only_on_filter_change() -> Result<(), String>
    {
        let mut config = EngineConfig::new(
            Root::of::<RememberedPosters>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json")))?),
        );
        config.viewport = size(px(800.0), px(600.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("Poster 0")?;
        app.scroll("id:personal-posters", px(1664.0))?;
        app.press("Leave posters")?;
        app.wait_for_gone("id:personal-posters", Duration::from_secs(1))?;
        app.press("Return posters")?;
        app.wait_for("id:personal-posters")?;
        app.scroll("id:personal-posters", px(-1664.0))?;
        assert!(app.scroll("id:personal-posters", px(-1.0)).is_err());
        app.scroll("id:personal-posters", px(1664.0))?;
        app.press("Filter posters")?;
        app.wait(Duration::from_millis(32))?;
        assert!(app.scroll("id:personal-posters", px(-1.0)).is_err());
        Ok(())
    }
}
