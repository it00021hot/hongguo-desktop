use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};
use std::rc::Rc;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct CreatorCard {
    pub id: String,
    pub title: String,
    pub cover: String,
    pub episodes: i64,
    pub hot: i64,
    pub plays: i64,
}

fn columns(width: f32) -> u32 {
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

pub(crate) fn creator_grid(
    items: Memo<Vec<CreatorCard>>,
    open: impl Fn(String) + 'static,
    prefetch: impl Fn(String) + 'static,
    load_more: impl Fn() + 'static,
) -> View {
    let open = Rc::new(open);
    let prefetch = Rc::new(prefetch);
    let width = signal(0.0_f32);
    let columns = computed(move || columns(window().width.0));
    view()
        .w_full()
        .mt_3()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            virtual_list(
                move || items.with(Vec::len),
                move |at| {
                    let item = computed(move || {
                        items.with(|items| items.get(at.index()).cloned().unwrap_or_default())
                    });
                    let hovered = signal(false);
                    let prefetch = prefetch.clone();
                    let open = open.clone();
                    view()
                        .w_full()
                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| {
                            hovered.set(true);
                            prefetch(item.get_untracked().id);
                        })
                        .on_mouse_leave(move |_: &nsh_core::MouseEvent| hovered.set(false))
                        .child(card(item, hovered).on_press(move || open(item.get_untracked().id)))
                },
            )
            .test_id("creator-works")
            .viewport(nsh_core::ListViewport::Parent)
            .columns(columns)
            .column_gap(px(12.0))
            .row_gap(px(12.0))
            .keys(move || {
                items.with(|items| items.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
            })
            .estimated_item_size(move || {
                px(
                    ((width.get() - (columns.get() as f32 - 1.0) * 12.0) / columns.get() as f32)
                        .max(0.0)
                        * 4.0
                        / 3.0,
                )
            })
            .on_load_more(load_more),
        )
}

fn card(item: Memo<CreatorCard>, hovered: Signal<bool>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || Some(item.get().title.into()))
        .w_full()
        .p_0()
        .min_h(px(0.0))
        .overflow_hidden()
        .rounded(super::super::radii::card())
        .bg(move || theme().color("poster-surface").expect("poster surface"))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(
            view()
                .relative()
                .w_full()
                .aspect_ratio(3.0 / 4.0)
                .child(when(
                    move || !item.get().cover.is_empty(),
                    move || {
                        image(move || ImageSrc::from(item.get().cover))
                            .absolute()
                            .inset_0()
                            .object_fit(nsh_core::ResizeMode::Cover)
                            .opacity(move || if hovered.get() { 0.8_f32 } else { 1.0_f32 })
                            .transition()
                            .duration(move || {
                                if reduce_motion() {
                                    0.0
                                } else {
                                    super::super::motion::FAST_MS
                                }
                            })
                            .accessible(false)
                    },
                ))
                .child(when(
                    move || item.get().cover.is_empty(),
                    move || view().absolute().inset_0().bg(move || theme().muted),
                ))
                .child(
                    view()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .p_2p5()
                        .pt_8()
                        .pointer_events(nsh_core::PointerEvents::None)
                        .bg_linear_to_t()
                        .gradient_from(move || {
                            theme()
                                .color("video-surface")
                                .expect("video surface")
                                .opacity(0.85)
                        })
                        .gradient_to(move || {
                            theme()
                                .color("video-surface")
                                .expect("video surface")
                                .opacity(0.0)
                        })
                        .child(
                            text(move || item.get().title)
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .number_of_lines(2)
                                .text_color(move || {
                                    theme().color("video-caption").expect("video caption")
                                }),
                        )
                        .child(metadata(item)),
                ),
        )
}

fn metadata(item: Memo<CreatorCard>) -> View {
    view()
        .mt_1()
        .flex_row()
        .items_center()
        .gap_2()
        .text_color(move || {
            theme()
                .color("video-caption")
                .expect("video caption")
                .opacity(0.7)
        })
        .child(when(
            move || item.get().episodes > 0,
            move || {
                text(move || {
                    crate::i18n::tr("detail.allEpisodesBadge")
                        .replace("{count}", &item.get().episodes.to_string())
                })
                .text_xs()
                .font_weight(FontWeight::NORMAL)
                .flex_shrink(1.0)
            },
        ))
        .child(when(
            move || item.get().hot > 0,
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_0p5()
                    .flex_shrink(1.0)
                    .child(
                        super::super::icon(super::super::Icon::Home)
                            .size_3()
                            .accessible(false),
                    )
                    .child(
                        text(move || crate::i18n::format_count_precise(item.get().hot))
                            .text_xs()
                            .font_weight(FontWeight::NORMAL)
                            .flex_shrink(1.0),
                    )
            },
        ))
        .child(when(
            move || item.get().plays > 0,
            move || {
                text(move || {
                    crate::i18n::tr("detail.playsCount").replace(
                        "{count}",
                        &crate::i18n::format_count_precise(item.get().plays),
                    )
                })
                .text_xs()
                .font_weight(FontWeight::NORMAL)
                .flex_shrink(1.0)
            },
        ))
}

pub(crate) fn creator_skeleton_grid() -> View {
    let width = signal(0.0_f32);
    let columns = computed(move || columns(window().width.0));
    view()
        .test_id("creator-works-loading")
        .w_full()
        .mt_3()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            virtual_list(
                || 10_usize,
                |_| {
                    super::super::browse_loading::skeleton()
                        .w_full()
                        .aspect_ratio(3.0 / 4.0)
                        .rounded(super::super::radii::card())
                },
            )
            .viewport(nsh_core::ListViewport::Parent)
            .columns(columns)
            .column_gap(px(12.0))
            .row_gap(px(12.0))
            .estimated_item_size(move || {
                px(
                    ((width.get() - (columns.get() as f32 - 1.0) * 12.0) / columns.get() as f32)
                        .max(0.0)
                        * 4.0
                        / 3.0,
                )
            }),
        )
}
