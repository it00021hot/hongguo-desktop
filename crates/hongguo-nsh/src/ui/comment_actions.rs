use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn comment_notice(message: impl IntoReactive<SharedString>, reviews: bool) -> View {
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
                .text_color(move || {
                    if reviews || !is_desktop() {
                        theme().muted_foreground
                    } else {
                        theme().color("video-picker-muted").expect("notice")
                    }
                }),
        )
}

pub(crate) fn comment_link(
    label: impl IntoReactive<SharedString>,
    player: bool,
    size: f32,
) -> nsh_base::ButtonElement {
    super::controls::surface(super::ButtonVariant::Quiet)
        .self_start()
        .min_h_0()
        .h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .p_0()
        .gap_1()
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || {
            if player && is_desktop() {
                theme().color("video-picker-muted").expect("picker muted")
            } else {
                theme().muted_foreground
            }
        })
        .hover(move |style| {
            style
                .bg(move || theme().background.opacity(0.0))
                .text_color(move || {
                    if player && is_desktop() {
                        theme().color("video-picker-text").expect("picker text")
                    } else {
                        theme().foreground
                    }
                })
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
        .border_color(move || {
            if reviews || !is_desktop() {
                theme().border
            } else {
                theme().color("video-picker-cell").expect("thread border")
            }
        })
        .pl(px(if reviews { 12.0 } else { 10.0 }))
        .child(content)
}

pub(crate) fn comment_like(
    selected: Memo<bool>,
    count: Memo<i64>,
    player: bool,
    reply: bool,
) -> nsh_base::ButtonElement {
    super::controls::surface(super::ButtonVariant::Quiet)
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
            } else if player && is_desktop() {
                theme().color("video-picker-muted").expect("muted")
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
                    } else if player && is_desktop() {
                        theme().color("video-controls").expect("controls")
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
                    "icons/heart-filled.svg"
                )))
                .size(size)
                .into_element()
            } else {
                super::icon(super::Icon::Liked).size(size).into_element()
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
