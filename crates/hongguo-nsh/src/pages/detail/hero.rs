use super::MetaData;
use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn hero(
    series: Memo<Option<hongguo_common::model::Series>>,
    meta: MetaData,
    upcoming: Memo<bool>,
) -> View {
    view()
        .flex_row()
        .gap(move || {
            px(if window().width.0 >= 768.0 {
                32.0
            } else {
                12.0
            })
        })
        .child(
            image(move || {
                nsh_core::element::ImageSrc::from(series.with(|series| {
                    series
                        .as_ref()
                        .map(|series| series.cover.clone())
                        .unwrap_or_default()
                }))
            })
            .w(move || {
                px(if window().width.0 >= 768.0 {
                    176.0
                } else {
                    112.0
                })
            })
            .flex_shrink_0()
            .aspect_ratio(3.0 / 4.0)
            .rounded(move || px(theme().radius.0 + 4.0))
            .bg(move || theme().muted)
            .object_fit(nsh_core::ResizeMode::Cover),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .pt_0p5()
                .child(
                    text(move || {
                        series.with(|series| {
                            series
                                .as_ref()
                                .map(|series| series.title.clone())
                                .unwrap_or_default()
                        })
                    })
                    .text_size(move || {
                        px(if window().width.0 >= 768.0 {
                            24.0
                        } else {
                            18.0
                        })
                    })
                    .line_height(move || {
                        px(if window().width.0 >= 768.0 {
                            33.0
                        } else {
                            24.75
                        })
                    })
                    .font_weight(FontWeight::BOLD),
                )
                .child(
                    text(move || {
                        let update = meta.with(|meta| {
                            meta.map(|meta| meta.update_tag.clone()).unwrap_or_default()
                        });
                        let episodes = series.with(|series| {
                            series.as_ref().map_or(0, |series| series.episode_count)
                        });
                        let hot = meta.with(|meta| meta.map_or(0, |meta| meta.hot_score));
                        if update.is_empty() && episodes == 0 && hot == 0 {
                            return String::new();
                        }
                        let mut summary = if update.is_empty() {
                            tr("detail.allEpisodesBadge").replace("{count}", &episodes.to_string())
                        } else {
                            update
                        };
                        if hot > 0 {
                            summary.push_str(" · ");
                            summary
                                .push_str(&tr("detail.heatValue").replace("{count}", &count(hot)));
                        }
                        summary
                    })
                    .mt_1p5()
                    .text_sm()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(move || theme().muted_foreground)
                    .style_when(
                        move || {
                            window().width.0 >= 768.0
                                || (meta.with(|meta| {
                                    meta.is_none_or(|meta| {
                                        meta.update_tag.is_empty() && meta.hot_score == 0
                                    })
                                }) && series.with(|series| {
                                    series
                                        .as_ref()
                                        .is_none_or(|series| series.episode_count == 0)
                                }))
                        },
                        |style| style.hidden(),
                    ),
                )
                .child(desktop_stats(meta))
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .gap_1p5()
                        .mt_3()
                        .child(when(
                            move || {
                                meta.with(|meta| meta.is_some_and(|meta| !meta.season.is_empty()))
                            },
                            move || {
                                ui::detail_badge(
                                    move || {
                                        meta.with(|meta| {
                                            meta.map(|meta| SharedString::from(meta.season.clone()))
                                                .unwrap_or_default()
                                        })
                                    },
                                    true,
                                )
                            },
                        ))
                        .child(
                            ui::detail_badge(
                                move || {
                                    let update = meta.with(|meta| {
                                        meta.map(|meta| SharedString::from(meta.update_tag.clone()))
                                            .unwrap_or_default()
                                    });
                                    if !update.is_empty() {
                                        update
                                    } else {
                                        tr("detail.allEpisodesBadge")
                                            .replace(
                                                "{count}",
                                                &series
                                                    .with(|series| {
                                                        series.as_ref().map_or(0, |series| {
                                                            series.episodes.len()
                                                        })
                                                    })
                                                    .to_string(),
                                            )
                                            .into()
                                    }
                                },
                                false,
                            )
                            .style_when(
                                move || {
                                    !upcoming.get()
                                        && series.with(|series| {
                                            series
                                                .as_ref()
                                                .is_none_or(|series| series.episodes.is_empty())
                                        })
                                },
                                |style| style.hidden(),
                            )
                            .bg(move || {
                                if upcoming.get() {
                                    theme().color("hot").expect("hot token")
                                } else {
                                    theme().secondary
                                }
                            })
                            .text_color(move || {
                                if upcoming.get() {
                                    theme().primary_foreground
                                } else {
                                    theme().secondary_foreground
                                }
                            }),
                        )
                        .child(each(
                            move || {
                                meta.with(|meta| {
                                    meta.map(|meta| meta.tags.clone()).unwrap_or_default()
                                })
                            },
                            |tag: &String| tag.clone(),
                            move |tag, _| {
                                ui::detail_badge(move || SharedString::from(tag.get()), false)
                            },
                        )),
                ),
        )
}

fn desktop_stats(meta: MetaData) -> View {
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1()
        .mt_2()
        .text_sm()
        .line_height(px(20.0))
        .font_weight(FontWeight::NORMAL)
        .text_color(move || theme().muted_foreground)
        .style_when(move || window().width.0 < 768.0, |style| style.hidden())
        .child(when(
            move || meta.with(|meta| meta.is_some_and(|meta| meta.hot_score > 0)),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        ui::icon(ui::Icon::Home)
                            .size_4()
                            .text_color(move || theme().color("hot").expect("hot token")),
                    )
                    .child(text(move || {
                        let value = meta.with(|meta| meta.map_or(0, |meta| meta.hot_score));
                        tr("detail.heatValue").replace("{count}", &count(value))
                    }))
            },
        ))
        .child(desktop_stat(meta, "detail.followCount", |meta| {
            meta.followed_cnt
        }))
        .child(desktop_stat(meta, "detail.playsCount", |meta| {
            meta.play_cnt
        }))
}

fn desktop_stat(
    meta: MetaData,
    key: &'static str,
    select: fn(&hongguo_api::detail::SeriesMeta) -> i64,
) -> View {
    let value = computed(move || meta.with(|meta| meta.map_or(0, select)));
    view()
        .style_when(move || value.get() <= 0, |style| style.hidden())
        .child(text(move || {
            tr(key).replace("{count}", &count(value.get()))
        }))
}

fn count(value: i64) -> String {
    let (value, key) = if crate::i18n::is_chinese() {
        if value >= 100_000_000 {
            (value as f64 / 100_000_000.0, "number.yi")
        } else if value >= 10_000 {
            (value as f64 / 10_000.0, "number.wan")
        } else {
            return value.to_string();
        }
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "number.million")
    } else if value >= 1000 {
        (value as f64 / 1000.0, "number.thousand")
    } else {
        return value.to_string();
    };
    let value = format!("{value:.1}");
    tr(key).replace("{value}", value.strip_suffix(".0").unwrap_or(&value))
}
