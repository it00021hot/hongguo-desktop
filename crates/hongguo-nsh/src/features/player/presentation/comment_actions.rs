use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn comment_more(
    label: impl IntoReactive<SharedString>,
    pending: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let pending = pending.into_reactive();
    let pending = computed(move || pending.get());
    crate::ui::controls::surface_height(crate::ui::ButtonVariant::Quiet, 32.0)
        .self_center()
        .my_3()
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_3()
        .py_0()
        .gap(px(6.0))
        .border_1()
        .border_color(move || {
            if dark() {
                theme().input
            } else {
                theme().border
            }
        })
        .bg(move || {
            if dark() {
                theme().input.opacity(0.3)
            } else {
                theme().background
            }
        })
        .hover(|style| {
            style.bg(move || {
                if dark() {
                    theme().input.opacity(0.5)
                } else {
                    theme().accent
                }
            })
        })
        .disabled(move || pending.get())
        .child(when(
            move || pending.get(),
            move || crate::ui::loading::spinner().size(px(14.0)).mr_1(),
        ))
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}

pub(crate) fn comment_notice(message: impl IntoReactive<SharedString>, _reviews: bool) -> View {
    let message = message.into_reactive();
    let visible = message.clone();
    view()
        .py_10()
        .items_center()
        .style_when(move || visible.get().is_empty(), |style| style.hidden())
        .child(
            text(move || message.get())
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .text_color(move || theme().muted_foreground),
        )
}

pub(crate) fn comment_link(
    label: impl IntoReactive<SharedString>,
    _player: bool,
    size: f32,
) -> nsh_base::ButtonElement {
    crate::ui::controls::surface(crate::ui::ButtonVariant::Quiet)
        .self_start()
        .min_h_0()
        .h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .p_0()
        .gap_1()
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().muted_foreground)
        .hover(move |style| {
            style
                .bg(move || theme().background.opacity(0.0))
                .text_color(move || theme().foreground)
        })
        .child(
            text(label)
                .text_size(move || px(if is_desktop() { size } else { 14.0 }))
                .font_weight(FontWeight::NORMAL),
        )
}

pub(crate) fn comment_thread_row(content: impl IntoElement, reviews: bool) -> View {
    view()
        .ml(px(if reviews { 48.0 } else { 42.0 }))
        .border_l(px(2.0))
        .border_color(move || theme().border)
        .pl(px(if reviews { 12.0 } else { 10.0 }))
        .child(content)
}

pub(crate) fn comment_like(
    selected: Memo<bool>,
    count: Memo<i64>,
    _player: bool,
    reply: bool,
) -> nsh_base::ButtonElement {
    crate::ui::controls::surface(crate::ui::ButtonVariant::Quiet)
        .flex_col()
        .gap(px(2.0))
        .min_h_0()
        .min_w_0()
        .p_0()
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .h(move || {
            px(if is_desktop() {
                if count.get() > 0 { 30.0 } else { 16.0 }
            } else {
                48.0
            })
        })
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || {
            if selected.get() {
                theme().color("hot").expect("hot")
            } else {
                theme().muted_foreground
            }
        })
        .hover(move |style| {
            style
                .bg(move || theme().background.opacity(0.0))
                .text_color(move || {
                    if selected.get() {
                        theme().color("hot").expect("hot")
                    } else {
                        theme().foreground
                    }
                })
        })
        .accessibility_label(move || {
            Some(
                format!(
                    "{} {}",
                    tr(if selected.get() {
                        "player.interact.liked"
                    } else {
                        "player.interact.like"
                    }),
                    count.get()
                )
                .into(),
            )
        })
        .child(dynamic(move || {
            let size = px(if !is_desktop() {
                24.0
            } else if reply {
                12.0
            } else {
                16.0
            });
            if selected.get() {
                svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui/icons/heart-filled.svg")
                )))
                .size(size)
                .into_element()
            } else {
                crate::ui::icon(crate::ui::Icon::Liked)
                    .size(size)
                    .into_element()
            }
        }))
        .child(when(
            move || count.get() > 0,
            move || {
                text(move || count.get().to_string())
                    .text_size(move || {
                        px(if !is_desktop() {
                            12.0
                        } else if reply {
                            9.0
                        } else {
                            10.0
                        })
                    })
                    .font_weight(FontWeight::NORMAL)
            },
        ))
}
