use super::{ButtonVariant, controls::surface};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

/// The narrow detail bar sits six points above the desktop's 50-point BottomNav.
pub(crate) fn detail_mobile_actions(
    collected: Memo<bool>,
    liked: Memo<bool>,
    busy: Memo<bool>,
    collect: impl Fn() + 'static,
    like: impl Fn() + 'static,
) -> View {
    view()
        .test_id("detail-mobile-actions")
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(6.0))
        .z_index(30)
        .flex_row()
        .gap_2()
        .px_4()
        .py_2p5()
        .border_t_1()
        .border_color(move || theme().border)
        .bg(move || theme().background.opacity(0.8))
        .backdrop_blur(px(8.0))
        .child(action(collected, busy, true).on_press(collect))
        .child(action(liked, busy, false).on_press(like))
}

fn action(selected: Memo<bool>, busy: Memo<bool>, collection: bool) -> nsh_base::ButtonElement {
    let markup = if collection {
        include_str!("icons/star.svg")
    } else {
        include_str!("icons/heart.svg")
    };
    surface(ButtonVariant::Quiet)
        .flex_1()
        .flex_row()
        .gap_2()
        .px_3()
        .py_2()
        .h(move || px(if is_desktop() { 40.0 } else { 48.0 }))
        .min_h(move || px(if is_desktop() { 40.0 } else { 48.0 }))
        .border_1()
        .border_color(move || theme().input)
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .shadow_xs()
        .hover(|style| {
            style.bg(move || {
                if dark() {
                    theme().input.opacity(0.5)
                } else {
                    theme().accent
                }
            })
        })
        .selected(selected)
        .selected_style(move |style| {
            style.text_color(move || {
                theme()
                    .color(if collection { "star" } else { "hot" })
                    .expect("social token")
            })
        })
        .disabled(busy)
        .child(
            svg(ImageSrc::svg_markup(markup))
                .fill_none()
                .style_when(selected, |style| style.fill_current())
                .stroke_current()
                .stroke_width(px(2.0))
                .size_4()
                .accessible(false),
        )
        .child(
            text(move || {
                tr(if collection {
                    if selected.get() {
                        "detail.collected"
                    } else {
                        "detail.collect"
                    }
                } else {
                    "player.interact.like"
                })
            })
            .text_sm()
            .font_weight(FontWeight::MEDIUM),
        )
}
