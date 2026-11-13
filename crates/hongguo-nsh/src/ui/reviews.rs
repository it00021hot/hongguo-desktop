use super::Icon;
use crate::i18n::tr;
use hongguo_api::danmaku::CommentTagStat;
use nsh_base::theme;
use nsh_core::prelude::*;

fn action_surface() -> nsh_base::ButtonElement {
    super::controls::surface(super::ButtonVariant::Quiet)
        .flex_row()
        .min_h_0()
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 16.0 } else { 48.0 }))
        .px_0()
        .py_0()
        .gap_1()
        .bg(move || theme().background.opacity(0.0))
        .text_color(move || theme().muted_foreground)
        .hover(|style| {
            style
                .bg(move || theme().background.opacity(0.0))
                .text_color(move || theme().foreground)
        })
        .selected_style(|style| {
            style
                .border(px(0.0))
                .bg(move || theme().background.opacity(0.0))
                .text_color(move || theme().color("hot").expect("hot token"))
        })
}

pub(crate) fn review_link(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    action_surface().child(text(label).text_xs().font_weight(FontWeight::NORMAL))
}

pub(crate) fn review_like(
    selected: impl IntoReactive<bool>,
    count: impl IntoReactive<i64>,
) -> nsh_base::ButtonElement {
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let count = count.into_reactive();
    let name = count.clone();
    action_surface()
        .selected(selected)
        .accessibility_label(move || {
            Some(
                format!(
                    "{} {}",
                    tr(if selected.get() {
                        "player.interact.liked"
                    } else {
                        "player.interact.like"
                    }),
                    name.get()
                )
                .into(),
            )
        })
        .child(dynamic(move || {
            if selected.get() {
                svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
                    "icons/heart-filled.svg"
                )))
                .size(px(14.0))
                .into_element()
            } else {
                super::icon(Icon::Liked).size(px(14.0)).into_element()
            }
        }))
        .child(when(
            {
                let count = count.clone();
                move || count.get() > 0
            },
            move || {
                text({
                    let count = count.clone();
                    move || count.get().to_string()
                })
                .text_xs()
                .font_weight(FontWeight::NORMAL)
            },
        ))
}

pub(crate) fn review_card() -> View {
    view()
        .p_3()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card.opacity(0.6))
}

#[derive(Clone, Copy)]
pub(crate) struct ReviewFocus(pub [ElementRef; 5]);

pub(crate) fn review_rating(
    stars: Signal<u8>,
    pending: Signal<bool>,
    focus: ReviewFocus,
) -> impl IntoElement {
    dynamic(move || {
        provide_context(focus);
        view()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                text(move || tr("detail.myRating"))
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                nsh_base::radio_group().child(
                    review_options()
                        .stars(move || u32::from(stars.get()))
                        .pending(pending)
                        .on_change(move |value: &u32| stars.set(*value as u8)),
                ),
            )
            .child(when(
                move || stars.get() > 0,
                move || {
                    text(move || tr("detail.ratedSuffix"))
                        .text_xs()
                        .text_color(move || theme().muted_foreground)
                },
            ))
    })
}

#[derive(Debug, Clone, PartialEq, Props)]
struct ReviewOptionsProps {
    stars: Prop<u32>,
    pending: Prop<bool>,
}

#[derive(Debug, Clone, PartialEq, Event)]
enum ReviewOptionsEvent {
    Change(u32),
}

// Keep construction inside the mounted group's scope, including when the
// group crosses the native component registry boundary.
#[component]
fn ReviewOptions(props: ReviewOptionsProps, cx: Scope<ReviewOptionsEvent>) -> impl IntoElement {
    let focus = use_context::<ReviewFocus>().expect("review focus owner");
    let hovered = signal(0_u32);
    let stars = props.stars;
    let pending = props.pending;
    view()
        .flex_row()
        .gap_0p5()
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| hovered.set(0))
        .children((1..=5_u32).map(move |value| {
            let node = focus.0[value as usize - 1];
            view()
                .on_mouse_enter(move |_: &nsh_core::MouseEvent| hovered.set(value))
                .child(
                    nsh_base::radio()
                        .element_ref(&node)
                        .accessibility_label(move || {
                            Some(
                                tr("detail.rateStar")
                                    .replace("{n}", &value.to_string())
                                    .into(),
                            )
                        })
                        .checked(move || stars.get() == value)
                        .disabled(pending)
                        .on_change(move |_: &bool| {
                            cx.emit(ReviewOptionsEvent::Change(value));
                            hovered.set(0);
                            after_answer(move || node.focus());
                        })
                        .p_0p5()
                        .min_h(move || px(if is_desktop() { 24.0 } else { 48.0 }))
                        .min_w(move || px(if is_desktop() { 24.0 } else { 48.0 }))
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
                        .child(dynamic(move || {
                            star(
                                value
                                    <= if hovered.get() > 0 {
                                        hovered.get()
                                    } else {
                                        stars.get()
                                    },
                                false,
                            )
                            .size_5()
                        })),
                )
        }))
}

fn star(filled: bool, half: bool) -> Svg {
    let glyph = (if half {
        svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
            "icons/star-half.svg"
        )))
    } else if filled {
        svg(nsh_core::element::ImageSrc::svg_markup(include_str!(
            "icons/star-filled.svg"
        )))
    } else {
        super::icon(Icon::Collections)
    })
    .text_color(move || {
        if filled || half {
            theme().color("star").expect("star token").opacity(1.0)
        } else {
            theme().muted_foreground.opacity(0.4)
        }
    });
    if filled || half {
        glyph.fill_current()
    } else {
        glyph.fill_none()
    }
}

pub(crate) fn review_stars(score: impl IntoReactive<String>) -> impl IntoElement {
    let score = score.into_reactive();
    view()
        .role(nsh_core::AccessibilityRole::Image)
        .label({
            let score = score.clone();
            move || {
                tr("detail.rateStar").replace(
                    "{n}",
                    &format!("{}", score.get().parse::<f64>().unwrap_or_default() / 2.0),
                )
            }
        })
        .child(dynamic(move || {
            let value = (score.get().parse::<f64>().unwrap_or_default() / 2.0).clamp(0.0, 5.0);
            view()
                .flex_row()
                .gap(px(1.0))
                .children((1..=5).map(move |at| {
                    star(
                        f64::from(at) <= value.floor(),
                        f64::from(at) == value.floor() + 1.0 && value.fract() >= 0.5,
                    )
                    .size_3()
                }))
        }))
}

pub(crate) fn review_summary(
    score: impl IntoReactive<String>,
    count: impl IntoReactive<i64>,
    tags: impl IntoReactive<Vec<CommentTagStat>>,
) -> impl IntoElement {
    let score = score.into_reactive();
    let count = count.into_reactive();
    let tags = tags.into_reactive();
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_2()
        .px_4()
        .py_3()
        .rounded_lg()
        .border_1()
        .border_color(move || theme().border)
        .child(when(
            {
                let score = score.clone();
                move || !score.get().is_empty()
            },
            {
                let score = score.clone();
                move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text({
                                let score = score.clone();
                                move || {
                                    score
                                        .get()
                                        .parse::<f64>()
                                        .map(|value| format!("{value:.1}"))
                                        .unwrap_or_else(|_| score.get())
                                }
                            })
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(move || theme().color("star").expect("star token")),
                        )
                        .child(review_stars(score.clone()))
                }
            },
        ))
        .child(
            text({
                let score = score.clone();
                move || {
                    if score.get().is_empty() {
                        tr("detail.noRating")
                    } else {
                        tr("detail.ratingCount")
                            .replace("{count}", &count.get().max(0).to_string())
                            .into()
                    }
                }
            })
            .text_sm()
            .text_color(move || theme().muted_foreground),
        )
        .child(each(
            move || tags.get(),
            |tag: &CommentTagStat| tag.tag_name.clone(),
            move |tag, _| {
                text(move || format!("{} {}", tag.get().tag_name, tag.get().count))
                    .text_xs()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(move || theme().muted)
            },
        ))
}
