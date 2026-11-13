use super::{Icon, icon, radii};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub(crate) struct NewDramaCard {
    pub id: String,
    pub title: String,
    pub cover: String,
    pub heat: String,
    pub category: String,
    pub episodes: u32,
    pub score: f64,
    pub downloaded: usize,
}

pub(crate) fn new_drama_grid(
    items: impl IntoReactive<Vec<NewDramaCard>>,
    finished: impl IntoReactive<bool>,
    open: impl Fn(String) + 'static,
    load_more: impl Fn() + 'static,
    audience: impl IntoReactive<i64>,
) -> impl IntoElement {
    let items = items.into_reactive();
    let finished = finished.into_reactive();
    let audience = audience.into_reactive();
    new_drama_grid_surface()
        .items(move || items.get())
        .finished(move || finished.get())
        .audience(move || audience.get())
        .on_open(move |id: &String| open(id.clone()))
        .on_load_more(load_more)
}

#[derive(Debug, Clone, PartialEq, Props)]
struct NewDramaGridSurfaceProps {
    items: Prop<Vec<NewDramaCard>>,
    finished: Prop<bool>,
    audience: Prop<i64>,
}

#[derive(Debug, Clone, PartialEq, Event)]
enum NewDramaGridSurfaceEvent {
    Open(String),
    LoadMore,
}

#[component]
fn NewDramaGridSurface(
    props: NewDramaGridSurfaceProps,
    cx: Scope<NewDramaGridSurfaceEvent>,
) -> impl IntoElement {
    let items = computed(move || props.items.get());
    let finished = computed(move || props.finished.get());
    let width = crate::view_memory::scope("new-grid").signal("width", 0.0_f32);
    let columns = computed(move || columns(window().width.0));
    view()
        .flex_1()
        .min_h(px(0.0))
        .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .child(dynamic(move || {
            let audience = props.audience.get();
            let key = match audience {
                0 => "new.scroll:0",
                1 => "new.scroll:1",
                _ => "new.scroll:2",
            };
            crate::view_memory::remember_scroll(
                virtual_list(
                    move || items.with(Vec::len),
                    move |at| {
                        let item = computed(move || {
                            items.with(|rows| rows.get(at.index()).cloned().unwrap_or_default())
                        });
                        card(item).on_press(move || {
                            cx.emit(NewDramaGridSurfaceEvent::Open(
                                item.with(|item| item.id.clone()),
                            ))
                        })
                    },
                )
                .key(key)
                .test_id("new-recommend-grid")
                .on_load_more(move || {
                    if !finished.get() {
                        cx.emit(NewDramaGridSurfaceEvent::LoadMore);
                    }
                })
                .flex_1()
                .columns(columns)
                .column_gap(px(16.0))
                .row_gap(px(16.0))
                .keys(move || {
                    items.with(|rows| rows.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
                })
                .estimated_item_size(move || {
                    let columns = columns.get() as f32;
                    let card_width = (width.get() - (columns - 1.0) * 16.0) / columns;
                    px((card_width - 2.0).max(0.0) * 4.0 / 3.0 + 78.0)
                })
                .footer(view().pt_8().pb_2().items_center().child(when(
                    move || finished.get(),
                    move || {
                        text(move || tr("feed.end"))
                            .text_sm()
                            .line_height(px(20.0))
                            .text_color(move || theme().muted_foreground)
                    },
                ))),
                key,
                computed(move || audience),
            )
        }))
}

fn columns(width: f32) -> u32 {
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

fn card(item: Memo<NewDramaCard>) -> nsh_base::ButtonElement {
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
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::motion::FAST_MS
            }
        })
        .ease_out()
        .hover(|style| {
            style
                .bg(move || theme().card)
                .border_color(move || theme().foreground.opacity(0.3))
        })
        .active(|style| style.scale(1.0).opacity(0.8))
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
                    move || item.with(|item| !item.heat.is_empty()),
                    move || {
                        view()
                            .absolute()
                            .bottom_1()
                            .left_1()
                            .flex_row()
                            .items_center()
                            .gap_0p5()
                            .text_color(move || theme().color("star").expect("star token"))
                            .child(icon(Icon::Home).size_3())
                            .child(
                                text(move || item.with(|item| item.heat.clone()))
                                    .text_size(px(10.0))
                                    .line_height(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                    },
                ))
                .child(when(
                    move || item.with(|item| item.episodes > 0),
                    move || {
                        view()
                            .absolute()
                            .bottom_1()
                            .right_1()
                            .px_1()
                            .py_0p5()
                            .rounded(px(4.0))
                            .bg(move || {
                                theme()
                                    .color("video-caption-background")
                                    .expect("caption background token")
                            })
                            .text_color(move || {
                                theme()
                                    .color("video-caption")
                                    .expect("caption foreground token")
                            })
                            .child(
                                text(move || {
                                    tr("newDrama.episodeTotal").replace(
                                        "{count}",
                                        &item.with(|item| item.episodes.to_string()),
                                    )
                                })
                                .text_size(px(10.0))
                                .line_height(px(15.0)),
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
                .p_2()
                .gap_1()
                .min_w(px(0.0))
                .child(
                    text(move || item.with(|item| item.title.clone()))
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1),
                )
                .child(
                    text(move || item.with(|item| item.category.clone()))
                        .text_xs()
                        .line_height(px(16.0))
                        .text_color(move || theme().muted_foreground)
                        .number_of_lines(1),
                )
                .child(when(
                    move || item.with(|item| item.score > 0.0),
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
                                icon(Icon::Collections)
                                    .size_3p5()
                                    .text_color(move || theme().color("star").expect("star token")),
                            )
                            .child(
                                text(move || {
                                    format!(
                                        "{:.1}{}",
                                        item.with(|item| item.score),
                                        tr("newDrama.scoreSuffix")
                                    )
                                })
                                .text_xs()
                                .line_height(px(16.0))
                                .font_weight(FontWeight::SEMIBOLD),
                            )
                    },
                )),
        )
}
