use super::{Icon, icon, motion::FAST_MS, radii};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn login_dialog(
    open: Memo<bool>,
    close: impl Fn() + Copy + 'static,
    content: impl Fn() -> View + 'static,
) -> View {
    view().child(when(open, move || {
        on_key_with(
            "escape",
            KeyOptions {
                when: nsh_core::keymap::When::Always,
                repeat: false,
                ..Default::default()
            },
            move |_| close(),
        );
        nsh_base::dialog()
            .animation(move || {
                if reduce_motion() {
                    ModalAnimation::None
                } else {
                    ModalAnimation::Fade
                }
            })
            .close_on_escape(true)
            .close_on_backdrop_press(true)
            .on_cancel(close)
            .child(
                view()
                    .flex_1()
                    .pb_keyboard()
                    .bg(move || {
                        theme()
                            .color("dialog-backdrop")
                            .expect("dialog backdrop token")
                    })
                    .child(
                        view().flex_1().items_center().justify_center().p_4().child(
                            view()
                                .test_id("login-dialog")
                                .relative()
                                .w(move || {
                                    px(if window().width.0 >= 640.0 {
                                        512.0
                                    } else {
                                        384.0
                                    })
                                })
                                .max_w(percentage(1.0))
                                .max_h(percentage(0.9))
                                .p_6()
                                .gap_4()
                                .rounded(move || theme().radius)
                                .border_1()
                                .border_color(move || theme().border)
                                .bg(move || theme().background)
                                .text_color(move || theme().foreground)
                                .shadow_lg()
                                .child(
                                    view()
                                        .gap_2()
                                        .child(
                                            text(move || tr("settings.loginTitle"))
                                                .text_lg()
                                                .line_height(nsh_core::LineHeight::Multiple(1.0))
                                                .font_weight(FontWeight::SEMIBOLD),
                                        )
                                        .child(
                                            text(move || tr("settings.loginPrompt"))
                                                .text_sm()
                                                .line_height(px(20.0))
                                                .text_color(move || theme().muted_foreground),
                                        ),
                                )
                                .child(content())
                                .child(
                                    view().absolute().top_4().right_4().child(
                                        nsh_base::button()
                                            .accessibility_label(move || Some(tr("common.close")))
                                            .size(px(16.0))
                                            .min_h(px(0.0))
                                            .p_0()
                                            .hit_slop(if is_desktop() { 8.0 } else { 16.0 })
                                            .rounded(px(2.0))
                                            .opacity(0.7)
                                            .hover(|style| style.opacity(1.0))
                                            .focus(|style| {
                                                style
                                                    .outline(px(2.0))
                                                    .outline_color(move || theme().ring)
                                            })
                                            .on_press(close)
                                            .child(icon(Icon::Close).size_4()),
                                    ),
                                ),
                        ),
                    ),
            )
    }))
}

pub(crate) fn login_input(label: impl IntoReactive<SharedString>) -> nsh_base::Input {
    let label = label.into_reactive();
    nsh_base::input()
        .label(move || label.get())
        .auto_correct(false)
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .px_3()
        .py_2()
        .text_sm()
        .rounded(radii::control)
        .border_1()
        .border_color(move || theme().input)
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().foreground)
        .caret_color(move || color(theme().ring))
        .selection_color(move || color(theme().primary.opacity(0.3)))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
}

pub(crate) fn login_send(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .px_4()
        .py_0()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .flex_shrink_0()
        .rounded(radii::control)
        .border_1()
        .border_color(move || theme().input)
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .text_color(move || theme().foreground)
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| style.opacity(0.8))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|style| style.opacity(0.5))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(icon(Icon::Send).size_4())
        .child(
            text(label)
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn account_badge(label: impl IntoReactive<SharedString>) -> View {
    view()
        .px_2()
        .py_0p5()
        .border_1()
        .border_color(move || theme().background.opacity(0.0))
        .rounded(radii::control)
        .bg(move || theme().secondary)
        .text_color(move || theme().secondary_foreground)
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn account_logout(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    super::library_action(label, Some(Icon::Logout))
}
