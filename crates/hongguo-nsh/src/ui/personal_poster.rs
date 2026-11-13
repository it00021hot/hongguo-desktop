//! Collection/liked cards have their own compact 10px text body.
use super::{ButtonVariant, Icon, controls::surface, icon, radii};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

pub(crate) fn personal_poster(
    title: impl IntoReactive<SharedString>,
    cover: impl IntoReactive<String>,
    metadata: impl IntoReactive<SharedString>,
    liked: bool,
) -> nsh_base::ButtonElement {
    let title = title.into_reactive();
    let name = title.clone();
    let cover = cover.into_reactive();
    let cover = computed(move || cover.get());
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(name.get()))
        .p_0()
        .min_h(px(0.0))
        .items_stretch()
        .justify_start()
        .overflow_hidden()
        .bg(move || theme().card)
        .rounded(radii::card)
        .border_1()
        .border_color(move || theme().border)
        .hover(|style| {
            style
                .bg(move || theme().card)
                .border_color(move || theme().foreground.opacity(0.3))
        })
        .focus(|style| style.border_color(move || theme().foreground.opacity(0.3)))
        .active(|style| style.scale(1.0))
        .selected_style(|style| {
            style
                .border_color(move || theme().primary.opacity(0.6))
                .bg(move || theme().primary.opacity(0.05))
        })
        .child(
            view()
                .relative()
                .w_full()
                .aspect_ratio(3.0 / 4.0)
                .bg(move || theme().muted)
                .child(when(
                    move || !cover.get().is_empty(),
                    move || {
                        image(move || ImageSrc::from(cover.get()))
                            .absolute()
                            .inset_0()
                            .object_fit(nsh_core::ResizeMode::Cover)
                    },
                ))
                .child(when(
                    move || cover.get().is_empty(),
                    move || {
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .text_color(move || theme().muted_foreground)
                            .child(
                                icon(if liked {
                                    Icon::Liked
                                } else {
                                    Icon::Collections
                                })
                                .size(px(20.0)),
                            )
                    },
                ))
                .child(
                    view()
                        .absolute()
                        .inset_0()
                        .pointer_events(nsh_core::PointerEvents::None)
                        .bg(move || {
                            theme()
                                .color("video-surface")
                                .expect("media surface token")
                                .opacity(0.0)
                        })
                        .group_hover(|style| {
                            style.bg(move || {
                                theme()
                                    .color("video-surface")
                                    .expect("media surface token")
                                    .opacity(0.3)
                            })
                        }),
                )
                .child(
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .pointer_events(nsh_core::PointerEvents::None)
                        .opacity(0.0)
                        .group_hover(|style| style.opacity(1.0))
                        .group_focus_within(|style| style.opacity(1.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_1p5()
                                .px_3()
                                .py_1p5()
                                .rounded_full()
                                .bg(move || theme().primary)
                                .text_color(move || theme().primary_foreground)
                                .child(icon(Icon::Play).size(px(14.0)))
                                .child(
                                    text(move || tr("history.continue"))
                                        .text_xs()
                                        .line_height(px(16.0))
                                        .font_weight(FontWeight::MEDIUM),
                                ),
                        ),
                ),
        )
        .child(
            view()
                .p_2p5()
                .gap_1()
                .child(
                    text(move || title.get())
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_color(move || theme().muted_foreground)
                        .children(if liked {
                            Some(
                                icon(Icon::Liked)
                                    .size(px(14.0))
                                    .fill_current()
                                    .text_color(move || theme().color("hot").expect("hot token")),
                            )
                        } else {
                            None
                        })
                        .child(
                            text(metadata)
                                .text_xs()
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL),
                        ),
                ),
        )
}

pub(crate) fn personal_poster_remove(
    label: impl IntoReactive<SharedString>,
    liked: bool,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(label.get()))
        .absolute()
        .top_2()
        .right_2()
        .p_0()
        .size(move || px(if is_desktop() { 28.0 } else { 48.0 }))
        .min_h(move || px(if is_desktop() { 28.0 } else { 48.0 }))
        .rounded_full()
        .bg(move || {
            theme()
                .color("video-surface")
                .expect("media surface token")
                .opacity(0.6)
        })
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("media controls token")
        })
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-surface")
                    .expect("media surface token")
                    .opacity(0.8)
            })
        })
        .style_when(is_desktop, |style| style.hidden())
        .group_hover(|style| style.flex())
        .group_focus_within(|style| style.flex())
        .child(icon(if liked { Icon::HeartOff } else { Icon::StarOff }).size(px(14.0)))
}

pub(crate) fn personal_poster_pick(checked: impl IntoReactive<bool>) -> View {
    let checked = checked.into_reactive();
    let checked = computed(move || checked.get());
    view()
        .absolute()
        .top_2()
        .left_2()
        .size(px(20.0))
        .rounded_full()
        .border_2()
        .border_color(move || {
            if checked.get() {
                theme().primary
            } else {
                theme().muted_foreground.opacity(0.5)
            }
        })
        .bg(move || {
            if checked.get() {
                theme().primary
            } else {
                theme().background.opacity(0.0)
            }
        })
        .text_color(move || theme().primary_foreground)
        .items_center()
        .justify_center()
        .pointer_events(nsh_core::PointerEvents::None)
        .child(when(checked, move || icon(Icon::Tasks).size(px(14.0))))
}

pub(crate) fn personal_poster_columns(width: f32) -> u32 {
    if width >= 1280.0 {
        5
    } else if width >= 1024.0 {
        4
    } else if width >= 640.0 {
        3
    } else {
        2
    }
}
