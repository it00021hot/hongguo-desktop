use super::{Icon, SeriesRowParts, icon, motion::FAST_MS, radii, series_row};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct CalendarRowData {
    pub(crate) title: String,
    pub(crate) cover: String,
    pub(crate) online: bool,
    pub(crate) heat: String,
    pub(crate) category: String,
    pub(crate) score: f64,
    pub(crate) episodes: u32,
    pub(crate) description: String,
    pub(crate) published: String,
    pub(crate) reserved: bool,
    pub(crate) pending: bool,
}

pub(crate) fn calendar_dates(count: impl IntoReactive<usize>, content: impl IntoElement) -> View {
    let count = count.into_reactive();
    let width = signal(0.0_f32);
    view()
        .w_full()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            nsh_base::radio_group()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .min_h(px(56.0))
                .w(move || {
                    let count = count.get();
                    px((count as f32 * 56.0 + count.saturating_sub(1) as f32 * 8.0)
                        .min(width.get()))
                })
                .child(content),
        )
}

pub(crate) fn calendar_date(
    weekday: impl IntoReactive<SharedString>,
    date: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
) -> nsh_base::RadioElement {
    let weekday = weekday.into_reactive();
    let date = date.into_reactive();
    let weekday = computed(move || weekday.get());
    let date = computed(move || date.get());
    let selected = selected.into_reactive();
    nsh_base::radio()
        .accessibility_label(move || Some(format!("{} {}", weekday.get(), date.get()).into()))
        .checked(move || selected.get())
        .flex_1()
        .min_w(px(56.0))
        .max_w(move || {
            px(if window().width.0 >= 640.0 {
                80.0
            } else {
                window().width.0
            })
        })
        .h(px(56.0))
        .px_1()
        .py_1p5()
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .checked_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
                .border_color(move || theme().primary.opacity(0.0))
        })
        .hover(move |style| {
            style
                .bg(move || theme().accent)
                .text_color(move || theme().accent_foreground)
        })
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(
            text(move || weekday.get())
                .text_size(px(11.0))
                .line_height(nsh_core::LineHeight::Multiple(1.25))
                .font_weight(FontWeight::NORMAL)
                .opacity(0.8),
        )
        .child(
            text(move || date.get())
                .text_sm()
                .line_height(nsh_core::LineHeight::Multiple(1.25))
                .font_weight(FontWeight::SEMIBOLD),
        )
}

pub(crate) fn calendar_row(
    data: impl IntoReactive<CalendarRowData>,
    open: impl Fn() + 'static,
    play: impl Fn() + 'static,
    reserve: impl Fn() + 'static,
) -> View {
    let data = data.into_reactive();
    let data = computed(move || data.get());
    let open = stored(Box::new(open) as Box<dyn Fn()>);
    let play = stored(Box::new(play) as Box<dyn Fn()>);
    let reserve = stored(Box::new(reserve) as Box<dyn Fn()>);
    series_row(
        move || SharedString::from(data.get().title),
        move || data.get().cover,
        SeriesRowParts {
            leading: None,
            content: view()
                .gap_1()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(
                            text(move || data.get().title)
                                .text_sm()
                                .line_height(px(20.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .number_of_lines(1),
                        )
                        .child(status_badge(move || data.get().online)),
                )
                .child(metadata(data))
                .child(when(
                    move || !data.get().description.is_empty(),
                    move || {
                        text(move || data.get().description)
                            .text_xs()
                            .line_height(nsh_core::LineHeight::Multiple(1.625))
                            .font_weight(FontWeight::NORMAL)
                            .number_of_lines(2)
                            .text_color(move || theme().muted_foreground.opacity(0.8))
                    },
                ))
                .into_element(),
            actions: Some(
                view()
                    .flex_shrink_0()
                    .gap_1p5()
                    .items_stretch()
                    .child(when(
                        move || !data.get().published.is_empty(),
                        move || {
                            text(move || data.get().published)
                                .text_xs()
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(move || theme().muted_foreground)
                        },
                    ))
                    .child(when(
                        move || data.get().online,
                        move || {
                            view()
                                .gap_1p5()
                                .items_stretch()
                                .child(
                                    action(move || tr("player.play"), false, || false)
                                        .child(icon(Icon::Play).size(px(14.0)))
                                        .child(
                                            text(move || tr("player.play"))
                                                .text_sm()
                                                .line_height(px(20.0))
                                                .font_weight(FontWeight::MEDIUM),
                                        )
                                        .on_press(move || play.with(|play| play())),
                                )
                                .child(
                                    nsh_base::button()
                                        .items_center()
                                        .justify_center()
                                        .text_color(move || theme().muted_foreground)
                                        .hover(|style| style.text_color(move || theme().foreground))
                                        .focus(|style| {
                                            style
                                                .outline(px(2.0))
                                                .outline_color(move || theme().ring)
                                        })
                                        .child(
                                            text(move || tr("rank.detail"))
                                                .text_xs()
                                                .line_height(px(16.0))
                                                .font_weight(FontWeight::NORMAL),
                                        )
                                        .on_press(move || open.with(|open| open())),
                                )
                        },
                    ))
                    .child(when(
                        move || !data.get().online,
                        move || {
                            action(
                                move || {
                                    tr(if data.get().reserved {
                                        "reservation.reserved"
                                    } else {
                                        "reservation.action"
                                    })
                                },
                                true,
                                move || data.get().reserved,
                            )
                            .disabled(move || data.get().reserved || data.get().pending)
                            .child(icon(Icon::Reservations).size_4())
                            .child(
                                text(move || {
                                    tr(if data.get().reserved {
                                        "reservation.reserved"
                                    } else {
                                        "reservation.action"
                                    })
                                })
                                .text_sm()
                                .line_height(px(20.0))
                                .font_weight(FontWeight::MEDIUM),
                            )
                            .on_press(move || reserve.with(|reserve| reserve()))
                        },
                    ))
                    .into_element(),
            ),
        },
        move || open.with(|open| open()),
    )
}

fn status_badge(online: impl IntoReactive<bool>) -> View {
    let online = online.into_reactive();
    let online = computed(move || online.get());
    view()
        .flex_shrink_0()
        .px_2()
        .py_0p5()
        .rounded(radii::control())
        .border_1()
        .border_color(move || theme().border.opacity(0.0))
        .bg(move || {
            if online.get() {
                theme()
                    .color("success")
                    .expect("success token")
                    .opacity(0.15)
            } else {
                theme().secondary
            }
        })
        .text_color(move || {
            if online.get() {
                theme().color("success").expect("success token")
            } else {
                theme().secondary_foreground
            }
        })
        .child(
            text(move || {
                tr(if online.get() {
                    "newDrama.online"
                } else {
                    "newDrama.upcoming"
                })
            })
            .text_size(px(10.0))
            .line_height(px(16.0))
            .font_weight(FontWeight::MEDIUM),
        )
}

fn metadata(data: Memo<CalendarRowData>) -> View {
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(when(
            move || !data.get().heat.is_empty(),
            move || {
                text(move || data.get().heat)
                    .text_xs()
                    .line_height(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().color("hot").expect("hot token"))
            },
        ))
        .child(when(
            move || !data.get().category.is_empty(),
            move || {
                text(move || data.get().category)
                    .text_xs()
                    .line_height(px(16.0))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(move || theme().muted_foreground)
            },
        ))
        .child(when(
            move || data.get().score > 0.0,
            move || super::personal_score(move || data.get().score),
        ))
        .child(when(
            move || data.get().episodes > 0,
            move || {
                text(move || {
                    tr("common.episodeCount").replace("{count}", &data.get().episodes.to_string())
                })
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL)
                .text_color(move || theme().muted_foreground)
            },
        ))
}

fn action(
    label: impl IntoReactive<SharedString>,
    reserve: bool,
    reserved: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let reserved = reserved.into_reactive();
    let reserved = computed(move || reserved.get());
    nsh_base::button()
        .accessibility_label(move || Some(label.get()))
        .h(px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_2p5()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_1()
        .rounded(move || if reserve { px(999.0) } else { radii::control() })
        .border_1()
        .border_color(move || {
            if reserve {
                theme().primary.opacity(0.0)
            } else {
                theme().input
            }
        })
        .bg(move || {
            if reserve {
                if reserved.get() {
                    theme().secondary
                } else {
                    theme().primary
                }
            } else if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .text_color(move || {
            if reserve {
                if reserved.get() {
                    theme().secondary_foreground
                } else {
                    theme().primary_foreground
                }
            } else {
                theme().foreground
            }
        })
        .hover(move |style| {
            style.opacity(if reserve { 0.9 } else { 1.0 }).bg(move || {
                if reserve {
                    if reserved.get() {
                        theme().secondary
                    } else {
                        theme().primary
                    }
                } else {
                    theme().accent
                }
            })
        })
        .active(|style| style.opacity(0.8))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|style| style.opacity(0.5))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}
