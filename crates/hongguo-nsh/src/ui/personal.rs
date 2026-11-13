use super::{ButtonVariant, Icon, controls::surface, icon, radii};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn personal_login_required(
    key: &'static str,
    glyph: Icon,
    on_login: impl Fn() + 'static,
) -> View {
    view()
        .py_20()
        .items_center()
        .gap_3()
        .text_color(move || theme().muted_foreground)
        .child(icon(glyph).size_8().opacity(0.4))
        .child(text(move || crate::i18n::tr(key)).text_sm())
        .child(
            surface(ButtonVariant::Primary)
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                .accessibility_label(move || crate::i18n::tr("settings.loginAction"))
                .on_press(on_login)
                .child(icon(Icon::Login).size_4())
                .child(
                    text(move || crate::i18n::tr("settings.loginAction"))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM),
                ),
        )
}

pub(crate) fn personal_tab(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_4()
        .py_1p5()
        .rounded_full()
        .bg(move || theme().muted)
        .text_color(move || theme().muted_foreground)
        .font_weight(FontWeight::NORMAL)
        .selected_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
                .border(px(0.0))
                .font_weight(FontWeight::MEDIUM)
        })
        .child(text(label).text_sm().line_height(px(20.0)))
}

pub(crate) fn personal_card(selected: impl IntoReactive<bool>) -> View {
    view()
        .flex_row()
        .items_stretch()
        .gap_4()
        .p_3()
        .rounded(radii::card)
        .overflow_hidden()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .ease_out()
        .hover(|style| style.border_color(move || theme().foreground.opacity(0.3)))
        .style_when(selected, |style| {
            style
                .border_color(move || theme().primary.opacity(0.6))
                .bg(move || theme().primary.opacity(0.05))
        })
}

pub(crate) fn personal_pick(
    label: impl IntoReactive<SharedString>,
    checked: impl IntoReactive<bool>,
) -> nsh_base::CheckboxElement {
    let label = label.into_reactive();
    let checked = checked.into_reactive();
    let checked = computed(move || checked.get());
    nsh_base::checkbox()
        .accessibility_label(move || Some(label.get()))
        .checked(checked)
        .flex_shrink_0()
        .indicator_style(|style| {
            style
                .size_5()
                .rounded_full()
                .items_center()
                .justify_center()
                .border_2()
                .border_color(move || theme().muted_foreground.opacity(0.5))
                .checked(|style| {
                    style
                        .bg(move || theme().primary)
                        .border_color(move || theme().primary)
                })
        })
        .indicator(when(checked, move || {
            icon(Icon::Tasks)
                .size(px(14.0))
                .text_color(move || theme().primary_foreground)
        }))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .hit_slop(if is_desktop() { 4.0 } else { 14.0 })
}

pub(crate) fn personal_cover(cover: impl IntoReactive<String>) -> nsh_base::ButtonElement {
    let cover = cover.into_reactive();
    surface(ButtonVariant::Quiet)
        .relative()
        .w(px(92.0))
        .h(px(92.0 * 4.0 / 3.0))
        .min_h(px(0.0))
        .flex_shrink_0()
        .p_0()
        .rounded(radii::control)
        .overflow_hidden()
        .bg(move || theme().muted)
        .active(|style| style.scale(1.0))
        .child(
            image(move || nsh_core::element::ImageSrc::from(cover.get()))
                .size_full()
                .object_fit(nsh_core::ResizeMode::Cover),
        )
}

pub(crate) fn personal_title(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(px(0.0))
        .min_w(px(0.0))
        .p_0()
        .self_start()
        .hover(|style| style.bg(move || theme().card))
        .active(|style| style.scale(1.0))
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .number_of_lines(1),
        )
}

pub(crate) fn personal_detail(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .min_h(px(0.0))
        .py_0()
        .px_1()
        .text_color(move || theme().muted_foreground)
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn personal_badge(label: impl IntoReactive<SharedString>, category: bool) -> View {
    view()
        .px_2()
        .py_0p5()
        .rounded(px(8.0))
        .border_1()
        .border_color(move || {
            if category {
                theme().secondary.opacity(0.0)
            } else {
                theme().border
            }
        })
        .bg(move || {
            if category {
                theme().secondary
            } else {
                theme().card
            }
        })
        .child(
            text(label)
                .text_size(px(10.0))
                .line_height(px(15.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn personal_score(score: impl IntoReactive<f64>) -> View {
    let score = score.into_reactive();
    view()
        .flex_row()
        .items_center()
        .gap_0p5()
        .flex_shrink_0()
        .text_color(move || theme().muted_foreground)
        .child(
            icon(Icon::Collections)
                .size_3()
                .text_color(move || theme().color("star").expect("star token")),
        )
        .child(
            text(move || format!("{:.1}", score.get()))
                .text_xs()
                .line_height(px(16.0)),
        )
}

pub(crate) fn personal_date(label: impl IntoReactive<SharedString>) -> View {
    view()
        .absolute()
        .bottom_0()
        .left_0()
        .px_1p5()
        .py_0p5()
        .pointer_events(nsh_core::PointerEvents::None)
        .bg(move || {
            theme()
                .color("video-surface")
                .expect("video surface token")
                .opacity(0.65)
        })
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls token")
        })
        .child(text(label).text_size(px(10.0)).line_height(px(15.0)))
}

pub(crate) fn personal_publish_at(label: impl IntoReactive<SharedString>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .text_color(move || theme().muted_foreground)
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
                "icons/calendar-clock.svg"
            )))
            .size_3()
            .fill_none()
            .stroke_current()
            .stroke_width(px(2.0)),
        )
        .child(text(label).text_xs().line_height(px(16.0)))
}
