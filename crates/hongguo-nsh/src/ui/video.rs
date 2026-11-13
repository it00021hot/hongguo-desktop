use super::{ButtonVariant, Icon, icon};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn player_stage() -> View {
    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .bg(move || theme().color("video-surface").expect("video surface token"))
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
        })
}

pub(crate) fn player_heat(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_xs()
        .line_height(px(16.0))
        .font_weight(FontWeight::NORMAL)
        .text_color(move || theme().color("video-heat").expect("video heat token"))
}

pub(crate) fn player_badge(label: impl IntoReactive<SharedString>) -> View {
    view()
        .px_1()
        .py(px(1.0))
        .rounded(px(2.0))
        .bg(move || {
            theme()
                .color("video-controls-hover")
                .expect("video hover token")
        })
        .child(
            text(label)
                .text_size(px(10.0))
                .line_height(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || {
                    theme()
                        .color("video-controls")
                        .expect("video controls token")
                }),
        )
}

pub(crate) fn player_campaign_badge(label: impl IntoReactive<SharedString>) -> View {
    player_badge(label).bg(move || theme().color("hot").expect("hot token").opacity(0.9))
}

pub(crate) fn player_binge(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    player_control(label)
        .w_full()
        .h_10()
        .min_h(px(40.0))
        .style_when(move || !is_desktop(), |style| style.h_12().min_h(px(48.0)))
        .rounded(super::radii::control)
        .bg(move || theme().color("video-binge").expect("video binge token"))
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-binge-hover")
                    .expect("video binge hover token")
            })
        })
}

pub(crate) fn player_control(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    player_surface().child(
        text(label)
            .text_sm()
            .line_height(px(20.0))
            .font_weight(FontWeight::NORMAL),
    )
}

pub(crate) fn player_phone_rate(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    player_surface()
        .min_h(move || px(if is_desktop() { 24.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 24.0 } else { 48.0 }))
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .p_0()
        .rounded_full()
        .child(
            view()
                .h(move || px(if is_desktop() { 24.0 } else { 32.0 }))
                .px_2p5()
                .items_center()
                .justify_center()
                .rounded_full()
                .pointer_events(PointerEvents::None)
                .bg(move || {
                    theme()
                        .color("video-surface")
                        .expect("video surface token")
                        .opacity(0.35)
                })
                .child(
                    text(label)
                        .text_xs()
                        .line_height(px(16.0))
                        .style_when(
                            move || !is_desktop(),
                            |style| style.text_sm().line_height(px(20.0)),
                        )
                        .font_family("monospace")
                        .font_weight(FontWeight::NORMAL),
                ),
        )
}

pub(crate) fn player_phone_time(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(10.0))
        .line_height(px(15.0))
        .style_when(
            move || !is_desktop(),
            |style| style.text_xs().line_height(px(16.0)),
        )
        .font_family("monospace")
        .font_weight(FontWeight::NORMAL)
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(0.7)
        })
}

pub(crate) fn player_title(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    player_surface()
        .min_h(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .h(auto())
        .flex_shrink(1.0)
        .min_w_0()
        .p_0()
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .min_w_0()
                .number_of_lines(move || if is_desktop() { 0_u32 } else { 2_u32 })
                .style_when(
                    move || !is_desktop(),
                    |style| style.text_base().line_height(px(24.0)),
                )
                .font_weight(FontWeight::SEMIBOLD),
        )
}

pub(crate) fn player_inline(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    player_surface()
        .min_h(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 20.0 } else { 48.0 }))
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .p_0()
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(0.6)
        })
        .hover(|style| {
            style.text_color(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls token")
            })
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn player_intro(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_xs()
        .line_height(px(20.0))
        .style_when(move || !is_desktop(), |style| style.text_sm())
        .font_weight(FontWeight::NORMAL)
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(0.7)
        })
}

pub(super) fn player_surface() -> nsh_base::ButtonElement {
    super::controls::surface(ButtonVariant::Quiet)
        .flex_shrink_0()
        .bg(move || {
            theme()
                .color("video-controls-hover")
                .expect("video hover token")
                .opacity(0.0)
        })
        .min_h(px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_2()
        .py_1()
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
        })
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-controls-hover")
                    .expect("video controls hover token")
            })
        })
        .selected_style(|style| {
            style.text_color(move || {
                theme()
                    .color("video-controls-selected")
                    .expect("video controls selected token")
            })
        })
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PlayerComposerVariant {
    Toolbar,
    Landscape,
}

pub(crate) fn player_send_surface(variant: Memo<PlayerComposerVariant>) -> View {
    view()
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .w(px(208.0))
        .ml_2()
        .style_when(
            move || variant.get() == PlayerComposerVariant::Landscape,
            |style| style.w_full().h_9().ml_0().pl_2p5(),
        )
        .min_w(px(0.0))
        .flex_row()
        .items_center()
        .gap_1()
        .pl_3()
        .pr_1()
        .rounded_full()
        .bg(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(if variant.get() == PlayerComposerVariant::Landscape {
                    0.1
                } else {
                    0.15
                })
        })
}

pub(crate) fn player_send_action(
    label: impl IntoReactive<SharedString>,
    variant: Memo<PlayerComposerVariant>,
) -> nsh_base::ButtonElement {
    player_surface()
        .min_h(move || {
            px(if variant.get() == PlayerComposerVariant::Landscape {
                28.0
            } else {
                24.0
            })
        })
        .h(move || {
            px(if variant.get() == PlayerComposerVariant::Landscape {
                28.0
            } else {
                24.0
            })
        })
        .px_2p5()
        .py_0()
        .rounded_full()
        .bg(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(0.15)
        })
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls token")
                    .opacity(0.25)
            })
        })
        .child(
            text(label)
                .text_size(move || {
                    px(if variant.get() == PlayerComposerVariant::Landscape {
                        11.0
                    } else {
                        12.0
                    })
                })
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn player_chrome(shown: impl IntoReactive<bool>) -> View {
    let shown = shown.into_reactive();
    let visible = shown.clone();
    view()
        .absolute()
        .inset_0()
        .pointer_events(move || {
            if visible.get() {
                nsh_core::PointerEvents::BoxNone
            } else {
                nsh_core::PointerEvents::None
            }
        })
        .opacity(move || if shown.get() { 1.0_f32 } else { 0.0_f32 })
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::CHROME_MS
            }
        })
        .ease_out()
        .child(when(
            move || !is_desktop() && window().height.0 > window().width.0,
            || {
                view()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h_80()
                    .pointer_events(PointerEvents::None)
                    .bg_linear_to_t()
                    .gradient_from(move || {
                        theme()
                            .color("video-surface")
                            .expect("video surface token")
                            .opacity(0.85)
                    })
                    .gradient_to(move || {
                        theme()
                            .color("video-surface")
                            .expect("video surface token")
                            .opacity(0.0)
                    })
            },
        ))
}

pub(crate) fn player_icon_button(
    glyph: Icon,
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    player_surface()
        .accessibility_label(move || label.get())
        .size(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .p_0()
        .child(super::player_hint(move || label.get()))
        .child(icon(glyph).size(px(16.0)))
}

pub(crate) fn player_privacy_button(active: Memo<bool>) -> nsh_base::ButtonElement {
    player_icon_button(Icon::Eye, move || crate::i18n::tr("player.incognito"))
        .selected(active)
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
                .opacity(if active.get() { 1.0 } else { 0.55 })
        })
        .selected_style(|style| {
            style
                .bg(move || {
                    theme()
                        .color("video-controls")
                        .expect("video controls token")
                        .opacity(0.0)
                })
                .text_color(move || {
                    theme()
                        .color("video-controls")
                        .expect("video controls token")
                })
        })
}

pub(crate) fn player_rail_item(
    glyph: Icon,
    label: impl IntoReactive<SharedString>,
    count: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    player_surface()
        .accessibility_label(move || label.get())
        .flex_col()
        .gap_0p5()
        .min_h(px(if is_desktop() { 0.0 } else { 48.0 }))
        .min_w(px(if is_desktop() { 44.0 } else { 48.0 }))
        .p_0()
        .child(rail_icon(glyph))
        .child(
            text(count)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}

fn rail_icon(glyph: Icon) -> Svg {
    let markup = match glyph {
        Icon::Liked => Some(include_str!("icons/douyin-heart.svg")),
        Icon::Collections => Some(include_str!("icons/douyin-star.svg")),
        Icon::Comments => Some(include_str!("icons/douyin-comment.svg")),
        Icon::Forward => Some(include_str!("icons/douyin-share.svg")),
        _ => None,
    };
    if let Some(markup) = markup {
        svg(nsh_core::element::ImageSrc::svg_markup(markup))
            .size(px(if matches!(glyph, Icon::Comments) {
                32.0
            } else {
                28.0
            }))
            .fill_current()
            .stroke_current()
    } else {
        icon(glyph).size(px(28.0))
    }
}

const CAPTION_WIDTH_RATIO: f32 = 0.85;

pub(crate) fn player_seek(
    value: Signal<f64>,
    maximum: impl IntoReactive<f64>,
) -> nsh_base::SliderElement {
    nsh_base::slider()
        .min(0.0)
        .max(maximum)
        .step(1.0)
        .accessibility_label("Playback position")
        .value(move || nsh_base::SliderValue::Single(value.get()))
        .on_change(move |next: &nsh_base::SliderValue| value.set(next.start()))
        .w_full()
        .min_h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
        .justify_center()
        .track_style(|style| {
            style
                .w_full()
                .h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
                .justify_center()
        })
        .indicator_style(|style| {
            style.w_full().h_0p5().rounded_full().bg(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls token")
                    .opacity(0.2)
            })
        })
        .range_style(|style| {
            style.h_0p5().rounded_full().bg(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls token")
                    .opacity(0.6)
            })
        })
        .thumb_class(if is_desktop() {
            "opacity-0 hover:opacity-100 focus:opacity-100"
        } else {
            "opacity-100"
        })
        .thumb_style(|style| {
            style.size(px(12.0)).rounded_full().bg(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls token")
            })
        })
}

pub(crate) fn video_caption(label: impl IntoReactive<SharedString>) -> View {
    video_caption_content(
        text(label)
            .text_base()
            .number_of_lines(1)
            .text_color(move || {
                theme()
                    .color("video-caption")
                    .expect("validated video caption foreground")
            }),
    )
}

pub(crate) fn video_caption_content(content: impl IntoElement) -> View {
    view()
        .self_start()
        .max_w(move || px(window().width.0 * CAPTION_WIDTH_RATIO))
        .px_2()
        .py_1()
        .rounded(move || theme().radius)
        .bg(move || {
            theme()
                .color("video-caption-background")
                .expect("validated video caption background")
        })
        .child(content)
}
