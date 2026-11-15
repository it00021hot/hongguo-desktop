use super::{ButtonVariant, button};
use crate::i18n::tr;
use nsh_base::{SheetEvent, theme};
use nsh_core::prelude::*;

const DESKTOP_WIDTH: f32 = 480.0;
const MOBILE_HEIGHT_RATIO: f32 = 0.82;
const BACKDROP_OPACITY: f32 = 0.8;
const CONTENT_PADDING: f32 = 16.0;

pub(crate) fn sheet<E: IntoElement>(
    open: Signal<bool>,
    title: impl IntoReactive<SharedString>,
    content: impl Fn() -> E + 'static,
) -> nsh_base::SheetElement {
    let title = title.into_reactive();
    nsh_base::sheet()
        .open(move || Some(open.get()))
        .side(move || {
            if is_desktop() {
                Placement::Right
            } else {
                Placement::Bottom
            }
        })
        .close_on_swipe(move || !is_desktop())
        .on_open_change(move |event: &SheetEvent| {
            if let SheetEvent::OpenChange(next, _) = event {
                open.set(*next);
            }
        })
        .backdrop_style(|style| style.bg(move || theme().background.opacity(BACKDROP_OPACITY)))
        .popup_style(|style| {
            style
                .w(move || {
                    px(if is_desktop() {
                        window().width.0.min(DESKTOP_WIDTH)
                    } else {
                        window().width.0
                    })
                })
                .h(move || {
                    px(window().height.0
                        * if is_desktop() {
                            1.0
                        } else {
                            MOBILE_HEIGHT_RATIO
                        })
                })
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .bg(move || theme().popover)
                .text_color(move || theme().popover_foreground)
        })
        .child(dynamic(move || {
            let label = title.clone();
            view().flex_1().pb_keyboard().child(
                view()
                    .flex_1()
                    .gap_3()
                    .pt(move || {
                        px(CONTENT_PADDING + if is_desktop() { safe_area().top.0 } else { 0.0 })
                    })
                    .pr_safe_area_plus(px(CONTENT_PADDING))
                    .pb_safe_area_plus(px(CONTENT_PADDING))
                    .pl_safe_area_plus(px(CONTENT_PADDING))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                text(move || label.get())
                                    .flex_1()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                button(move || tr("common.close"), ButtonVariant::Quiet)
                                    .on_press(move || open.set(false)),
                            ),
                    )
                    .child(content()),
            )
        }))
}
