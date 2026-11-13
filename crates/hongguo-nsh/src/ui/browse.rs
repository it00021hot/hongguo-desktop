use super::{Icon, icon, radii};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub(crate) struct BrowseCard {
    pub id: String,
    pub title: String,
    pub cover: String,
    pub heat: String,
    pub tags: Vec<String>,
    pub episodes: u32,
    pub downloaded: usize,
}

pub(crate) fn browse_search_action(busy: Memo<bool>) -> nsh_base::ButtonElement {
    super::controls::surface(super::ButtonVariant::Primary)
        .h_9()
        .px_3()
        .flex_row()
        .gap_2()
        .child(dynamic(move || {
            if busy.get() {
                super::browse_loading::browse_spinner().into_element()
            } else {
                icon(Icon::Search).into_element()
            }
        }))
        .child(
            text(move || tr("search.submit"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn browse_exit_action() -> nsh_base::ButtonElement {
    super::controls::surface(super::ButtonVariant::Quiet)
        .h_9()
        .px_3()
        .flex_row()
        .gap_2()
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
        .child(icon(Icon::Close))
        .child(
            text(move || tr("search.exit"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn browse_chip(label: impl IntoReactive<SharedString>) -> nsh_base::ToggleElement {
    nsh_base::toggle()
        .min_h(px(if is_desktop() { 22.0 } else { 48.0 }))
        .px_3()
        .py_0p5()
        .rounded_full()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().muted_foreground)
        .font_weight(FontWeight::NORMAL)
        .pressed_style(|style| {
            style
                .border_color(move || theme().primary)
                .bg(move || theme().primary.opacity(0.1))
                .text_color(move || theme().primary)
                .font_weight(FontWeight::MEDIUM)
        })
        .hover(|style| {
            style
                .bg(move || theme().accent)
                .text_color(move || theme().foreground)
        })
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .active(|style| style.opacity(0.8))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .child(text(label).text_xs().line_height(px(16.0)))
}

pub(crate) fn browse_filter_group() -> nsh_base::ToggleGroupElement {
    nsh_base::toggle_group()
        .flex_1()
        .flex_row()
        .flex_wrap()
        .gap_x_1()
        .gap_y_1p5()
}

pub(crate) fn browse_expander(
    expanded: Signal<bool>,
    count: Memo<usize>,
    toggle: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || tr("browse.filters"))
        .min_h(px(20.0))
        .flex_row()
        .items_center()
        .gap_1p5()
        .p_0()
        .self_start()
        .text_color(move || theme().muted_foreground)
        .hover(|style| style.text_color(move || theme().foreground))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .child(icon(Icon::Filters))
        .child(
            text(move || tr("browse.filters"))
                .text_sm()
                .line_height(px(20.0))
                .font_weight(FontWeight::NORMAL),
        )
        .child(when(
            move || count.get() > 0,
            move || {
                view()
                    .px_1p5()
                    .py_0p5()
                    .rounded(radii::control())
                    .border_1()
                    .border_color(move || theme().border.opacity(0.0))
                    .bg(move || theme().secondary)
                    .child(
                        text(move || count.get().to_string())
                            .text_size(px(10.0))
                            // Tauri's arbitrary 10px size inherits text-sm's 20/14 line height.
                            .line_height(px(10.0 * 20.0 / 14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || theme().secondary_foreground),
                    )
            },
        ))
        .child(icon(Icon::Dropdown).rotate(move || deg(if expanded.get() { 180.0 } else { 0.0 })))
        .on_press(toggle)
}

pub(crate) fn browse_grid(
    items: impl IntoReactive<Vec<BrowseCard>>,
    open: impl Fn(String) + 'static,
    load_more: impl Fn() + 'static,
) -> impl IntoElement {
    let items = items.into_reactive();
    browse_grid_surface()
        .items(move || items.get())
        .on_open(move |id: &String| open(id.clone()))
        .on_load_more(load_more)
}

#[derive(Debug, Clone, PartialEq, Props)]
struct BrowseGridSurfaceProps {
    items: Prop<Vec<BrowseCard>>,
}

#[derive(Debug, Clone, PartialEq, Event)]
enum BrowseGridSurfaceEvent {
    Open(String),
    LoadMore,
}

#[component]
fn BrowseGridSurface(
    props: BrowseGridSurfaceProps,
    cx: Scope<BrowseGridSurfaceEvent>,
) -> impl IntoElement {
    let items = computed(move || props.items.get());
    let width = signal(0.0_f32);
    let columns = computed(move || columns(window().width.0));
    view()
        .w_full()
        .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .child(
            virtual_list(
                move || items.with(Vec::len),
                move |at| {
                    let item = computed(move || {
                        items.with(|items| items.get(at.index()).cloned().unwrap_or_default())
                    });
                    card(item).on_press(move || {
                        cx.emit(BrowseGridSurfaceEvent::Open(
                            item.with(|item| item.id.clone()),
                        ))
                    })
                },
            )
            .test_id("browse-results")
            .viewport(nsh_core::ListViewport::Parent)
            .columns(columns)
            .column_gap(px(16.0))
            .row_gap(px(16.0))
            .on_load_more(move || cx.emit(BrowseGridSurfaceEvent::LoadMore))
            .keys(move || {
                items.with(|items| items.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
            })
            .estimated_item_size(move || {
                let columns = columns.get() as f32;
                px(
                    ((width.get() - (columns - 1.0) * 16.0) / columns - 2.0).max(0.0) * 4.0 / 3.0
                        + 94.0,
                )
            }),
        )
}

pub(super) fn columns(width: f32) -> u32 {
    if width >= 1536.0 {
        8
    } else if width >= 1024.0 {
        5
    } else if width >= 768.0 {
        4
    } else if width >= 640.0 {
        3
    } else {
        2
    }
}

fn card(item: Memo<BrowseCard>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || Some(item.with(|item| item.title.clone().into())))
        .w_full()
        .p_0()
        .items_stretch()
        .justify_start()
        .overflow_hidden()
        .rounded(radii::card)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .hover(|style| style.border_color(move || theme().foreground.opacity(0.3)))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .active(|style| style.opacity(0.8))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .child(
            view()
                .relative()
                .w_full()
                .aspect_ratio(3.0 / 4.0)
                .overflow_hidden()
                .bg(move || theme().muted)
                .child(
                    image(move || ImageSrc::from(item.with(|item| item.cover.clone())))
                        .absolute()
                        .inset_0()
                        .object_fit(nsh_core::ResizeMode::Cover),
                )
                .child(when(
                    move || item.with(|item| item.episodes > 0),
                    move || {
                        view()
                            .absolute()
                            .bottom_2()
                            .left_2()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.0))
                            .bg(move || {
                                theme()
                                    .color("video-caption-background")
                                    .expect("video caption background")
                            })
                            .text_color(move || {
                                theme().color("video-caption").expect("video caption")
                            })
                            .child(
                                text(move || {
                                    tr("common.episodeCount").replace(
                                        "{count}",
                                        &item.with(|item| item.episodes.to_string()),
                                    )
                                })
                                .text_xs()
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL),
                            )
                    },
                ))
                .child(when(
                    move || item.with(|item| item.downloaded > 0),
                    move || {
                        view()
                            .absolute()
                            .top_2()
                            .right_2()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(move || {
                                theme()
                                    .color("success")
                                    .expect("success token")
                                    .opacity(0.15)
                            })
                            .text_color(move || theme().color("success").expect("success token"))
                            .child(
                                text(move || item.with(|item| item.downloaded.to_string()))
                                    .text_xs()
                                    .line_height(px(16.0))
                                    .font_weight(FontWeight::MEDIUM),
                            )
                    },
                )),
        )
        .child(
            view()
                .p_3()
                .gap_1p5()
                .child(
                    text(move || item.with(|item| item.title.clone()))
                        .text_sm()
                        .line_height(px(21.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1),
                )
                .child(when(
                    move || item.with(|item| !item.heat.is_empty()),
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_0p5()
                            .text_color(move || theme().color("warm").expect("warm token"))
                            .child(icon(Icon::Home).size(px(14.0)))
                            .child(
                                text(move || item.with(|item| item.heat.clone()))
                                    .text_xs()
                                    .line_height(px(16.0))
                                    .font_weight(FontWeight::MEDIUM),
                            )
                    },
                ))
                .child(view().flex_row().gap_1().overflow_hidden().child(each(
                    move || item.with(|item| item.tags.iter().take(3).cloned().collect::<Vec<_>>()),
                    |tag: &String| tag.clone(),
                    move |tag, _| {
                        view()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(move || theme().secondary)
                            .child(
                                text(move || tag.get())
                                    .text_size(px(10.0))
                                    .line_height(px(15.0))
                                    .font_weight(FontWeight::MEDIUM),
                            )
                    },
                ))),
        )
}
