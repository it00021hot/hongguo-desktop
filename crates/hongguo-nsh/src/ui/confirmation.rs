use super::{ButtonVariant, button, motion::CHROME_MS};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn confirmation<E: IntoElement>(
    open: Signal<bool>,
    title: impl IntoReactive<SharedString>,
    action: impl IntoReactive<SharedString>,
    body: impl Fn() -> E + 'static,
    accept: impl Fn() + 'static,
) -> impl IntoElement {
    let title = title.into_reactive();
    let action = action.into_reactive();
    let accept = stored(accept);
    when(open, move || {
        let title = title.clone();
        let action = action.clone();
        nsh_base::dialog()
            .animation(move || {
                if reduce_motion() {
                    ModalAnimation::None
                } else {
                    ModalAnimation::Fade
                }
            })
            .close_on_escape(true)
            .close_on_backdrop_press(false)
            .on_cancel(move || open.set(false))
            .child(
                view()
                    .test_id("confirmation")
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .bg(move || {
                        theme()
                            .color("dialog-backdrop")
                            .expect("dialog backdrop token")
                    })
                    .child(
                        view()
                            .role(nsh_core::AccessibilityRole::Alert)
                            .label({
                                let title = title.clone();
                                move || title.get()
                            })
                            .w(px(512.0))
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
                            .transition()
                            .duration(move || if reduce_motion() { 0.0 } else { CHROME_MS })
                            .child(
                                view()
                                    .gap_2()
                                    .child(
                                        text({
                                            let title = title.clone();
                                            move || title.get()
                                        })
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(body()),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .justify_end()
                                    .gap_2()
                                    .child(cancel().on_press(move || open.set(false)))
                                    .child(
                                        button(move || action.get(), ButtonVariant::Primary)
                                            .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
                                            .py_0()
                                            .on_press(move || {
                                                open.set(false);
                                                accept.with(|accept| accept());
                                            }),
                                    ),
                            ),
                    ),
            )
    })
}

fn cancel() -> nsh_base::ButtonElement {
    nsh_base::button()
        .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .px_4()
        .py_0()
        .rounded(super::radii::control)
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
        .items_center()
        .justify_center()
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| {
            style.opacity(0.8).scale(move || {
                if reduce_motion() {
                    1.0
                } else {
                    super::motion::PRESS_SCALE
                }
            })
        })
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .child(
            text(move || tr("common.cancel"))
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::MEDIUM),
        )
}
