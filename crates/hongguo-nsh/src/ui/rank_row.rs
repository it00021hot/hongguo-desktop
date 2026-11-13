use super::{ButtonVariant, Icon, SeriesRowParts, controls::surface, icon, radii, series_row};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct RankRowData {
    pub(crate) title: String,
    pub(crate) cover: String,
    pub(crate) rank: u32,
    pub(crate) season: String,
    pub(crate) metadata: String,
    pub(crate) description: String,
    pub(crate) heat: String,
    pub(crate) secondary: String,
    pub(crate) upcoming: bool,
    pub(crate) reserved: bool,
    pub(crate) pending: bool,
}

pub(crate) fn rank_row(
    data: impl IntoReactive<RankRowData>,
    open: impl Fn() + 'static,
    play: impl Fn() + 'static,
    reserve: impl Fn() + 'static,
) -> View {
    let data = data.into_reactive();
    let data = computed(move || data.get());
    let open = stored(Box::new(open) as Box<dyn Fn()>);
    let reserve = stored(Box::new(reserve) as Box<dyn Fn()>);
    let play = stored(Box::new(play) as Box<dyn Fn()>);
    series_row(
        move || SharedString::from(data.get().title),
        move || data.get().cover,
        SeriesRowParts {
            leading: Some(
                when(
                    move || data.get().rank > 0,
                    move || {
                        text(move || data.get().rank.to_string())
                            .w_8()
                            .flex_shrink_0()
                            .text_center()
                            .text_2xl()
                            .font_weight(FontWeight::BLACK)
                            .accessible(false)
                            .text_color(move || {
                                if data.get().rank <= 3 {
                                    theme().color("star").expect("star token")
                                } else {
                                    theme().muted_foreground.opacity(0.5)
                                }
                            })
                    },
                )
                .into_element(),
            ),
            content: info(data).into_element(),
            actions: Some(
                view()
                    .flex_shrink_0()
                    .gap_1p5()
                    .items_stretch()
                    .child(when(
                        move || data.get().upcoming,
                        move || {
                            outline(
                                move || {
                                    tr(if data.get().reserved {
                                        "player.reserved"
                                    } else {
                                        "player.reserve"
                                    })
                                },
                                move || {
                                    if data.get().reserved {
                                        Icon::Check
                                    } else {
                                        Icon::Bell
                                    }
                                },
                            )
                            .disabled(move || data.get().pending)
                            .on_press(move || reserve.with(|reserve| reserve()))
                        },
                    ))
                    .child(when(
                        move || !data.get().upcoming,
                        move || {
                            view()
                                .gap_1p5()
                                .items_stretch()
                                .child(
                                    outline(move || tr("player.play"), || Icon::Play)
                                        .disabled(move || data.get().pending)
                                        .on_press(move || play.with(|play| play())),
                                )
                                .child(
                                    surface(ButtonVariant::Quiet)
                                        .min_h(px(16.0))
                                        .h_4()
                                        .p_0()
                                        .child(
                                            text(move || tr("rank.detail"))
                                                .text_xs()
                                                .line_height(px(16.0))
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(move || theme().muted_foreground),
                                        )
                                        .on_press(move || open.with(|open| open())),
                                )
                        },
                    ))
                    .into_element(),
            ),
        },
        move || open.with(|open| open()),
    )
}

fn info(data: Memo<RankRowData>) -> View {
    view()
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
                .child(when(
                    move || !data.get().season.is_empty(),
                    move || {
                        view()
                            .px_1()
                            .py_0p5()
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(move || theme().border)
                            .child(
                                text(move || data.get().season)
                                    .text_size(px(10.0))
                                    .line_height(px(16.0))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(move || theme().muted_foreground),
                            )
                    },
                )),
        )
        .child(when(
            move || !data.get().metadata.is_empty(),
            move || {
                text(move || data.get().metadata)
                    .text_xs()
                    .line_height(px(16.0))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(move || theme().muted_foreground)
                    .number_of_lines(1)
            },
        ))
        .child(when(
            move || !data.get().description.is_empty(),
            move || {
                text(move || data.get().description)
                    .text_xs()
                    .line_height(nsh_core::LineHeight::Multiple(1.625))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(move || theme().muted_foreground.opacity(0.8))
                    .number_of_lines(2)
            },
        ))
        .child(when(
            move || !data.get().heat.is_empty() || !data.get().secondary.is_empty(),
            move || {
                view()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(when(
                        move || !data.get().heat.is_empty(),
                        move || {
                            view()
                                .flex_row()
                                .gap_0p5()
                                .items_center()
                                .flex_shrink_0()
                                .text_color(move || theme().color("warm").expect("warm token"))
                                .child(icon(Icon::Home).size_3())
                                .child(
                                    text(move || data.get().heat)
                                        .text_xs()
                                        .line_height(px(16.0))
                                        .font_weight(FontWeight::NORMAL),
                                )
                        },
                    ))
                    .child(
                        text(move || data.get().secondary)
                            .text_xs()
                            .line_height(px(16.0))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(move || theme().muted_foreground)
                            .number_of_lines(1),
                    )
            },
        ))
}

fn outline(
    label: impl IntoReactive<SharedString>,
    glyph: impl IntoReactive<Icon>,
) -> nsh_base::ButtonElement {
    let glyph = glyph.into_reactive();
    surface(ButtonVariant::Quiet)
        .min_h(px(32.0))
        .h_8()
        .px_2p5()
        .py_1()
        .flex_row()
        .gap_1()
        .border_1()
        .border_color(move || theme().input)
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .child(dynamic(move || {
            let glyph = glyph.get();
            icon(glyph).size(px(if matches!(glyph, Icon::Filters) {
                16.0
            } else {
                14.0
            }))
        }))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn rank_sub_group() -> nsh_base::ToggleGroupElement {
    nsh_base::toggle_group().gap_1().items_start()
}

pub(crate) fn rank_sub_tab(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
) -> nsh_base::ToggleElement {
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    nsh_base::toggle()
        .pressed(selected)
        .items_center()
        .justify_center()
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .flex_shrink_0()
        .style_when(move || window().width.0 >= 768.0, |style| style.w_full())
        .px_3()
        .py_2()
        .rounded(radii::control())
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().muted_foreground)
        .hover(|style| {
            style
                .bg(move || theme().accent)
                .text_color(move || theme().foreground)
        })
        .pressed_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
        })
        .active(|style| {
            style.opacity(0.8).scale(move || {
                if reduce_motion() {
                    1.0
                } else {
                    super::motion::PRESS_SCALE
                }
            })
        })
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .ease_out()
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(
            text(label)
                .number_of_lines(1)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::NORMAL)
                .style_when(selected, |style| style.font_weight(FontWeight::MEDIUM)),
        )
}

pub(crate) fn rank_filter(
    label: impl IntoReactive<SharedString>,
    open: Signal<bool>,
) -> nsh_base::ButtonElement {
    outline(label, || Icon::Filters).gap_1p5().child(
        icon(Icon::Dropdown)
            .size_4()
            .rotate(move || deg(if open.get() { 180.0 } else { 0.0 })),
    )
}
