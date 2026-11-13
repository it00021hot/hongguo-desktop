use super::{Icon, field, icon, motion, radii};
use nsh_base::{OpenChangeReason, PopoverEvent, theme};
use nsh_core::{element::ImageSrc, prelude::*};
mod scroll;

#[derive(Clone, PartialEq)]
pub(crate) struct SearchSuggestion {
    pub word: SharedString,
    pub cover: String,
    pub description: SharedString,
    pub rich: bool,
    pub parts: Vec<(SharedString, bool)>,
}

#[derive(Clone, Copy)]
pub(crate) struct SearchPopup {
    pub open: Memo<bool>,
    pub composing: Signal<bool>,
    pub selected: Signal<usize>,
    pub options: Memo<Vec<SearchSuggestion>>,
}

pub(crate) fn search_field(label: impl IntoReactive<SharedString>) -> nsh_base::Input {
    field(label)
        .role(nsh_core::AccessibilityRole::Combobox)
        .submit_behavior(nsh_core::SubmitBehavior::Submit)
        .auto_correct(false)
}

pub(crate) fn search_suggestions(
    popup: SearchPopup,
    input: nsh_base::Input,
    pick: impl Fn(usize) + Copy + 'static,
    dismiss: impl Fn() + Copy + 'static,
) -> View {
    let width = signal(0.0_f32);
    view().flex_1().min_w_48().child(
        nsh_base::popover()
            .open(move || Some(popup.open.get()))
            .placement(Placement::Bottom)
            .align(Align::Start)
            .offset(px(6.0))
            .margin(px(8.0))
            .close_on_escape(move || !popup.composing.get())
            .trigger(
                view()
                    .w_full()
                    .focusable(false)
                    .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
                    .child(
                        input
                            .w_full()
                            .accessibility_state(move || AccessibilityState {
                                expanded: Some(popup.open.get()),
                                ..Default::default()
                            }),
                    ),
            )
            .on_open_change(move |event: &PopoverEvent| {
                if let PopoverEvent::OpenChange(false, reason) = event
                    && *reason != OpenChangeReason::TriggerPress
                {
                    dismiss();
                }
            })
            .popup_style(move |style| {
                style
                    .w(move || px(width.get()))
                    .max_w(move || px((window().width.0 - 16.0).max(0.0)))
                    .max_h(px(320.0))
                    .rounded(radii::card())
                    .border_1()
                    .border_color(move || theme().border)
                    .bg(move || theme().popover)
                    .text_color(move || theme().popover_foreground)
                    .shadow_md()
                    .origin_top_left()
                    .transition()
                    .duration(move || {
                        if reduce_motion() {
                            0.0
                        } else {
                            motion::FAST_MS
                        }
                    })
                    .ease_out()
            })
            .popup_starting_style(|style| {
                style.opacity(0.0).scale(move || {
                    if reduce_motion() {
                        1.0
                    } else {
                        motion::PRESS_SCALE
                    }
                })
            })
            .popup_ending_style(|style| {
                style.opacity(0.0).scale(move || {
                    if reduce_motion() {
                        1.0
                    } else {
                        motion::PRESS_SCALE
                    }
                })
            })
            .child(dynamic(move || {
                let scroll_state = scroll::PopupScroll::new(popup.selected);
                scroll()
                    .element_ref(&scroll_state.node)
                    .max_h(px(318.0))
                    .keyboard_taps(nsh_core::KeyboardTaps::Always)
                    .on_layout(move |bounds: &Bounds<Points>| scroll_state.layout(bounds))
                    .on_scroll(move |event: &nsh_core::element::ScrollEvent| {
                        scroll_state.moved(event)
                    })
                    .child(
                        view()
                            .test_id("browse-suggestions")
                            .role(nsh_core::AccessibilityRole::Listbox)
                            .label(move || crate::i18n::tr("search.placeholder"))
                            .p_1p5()
                            .child(each(
                                move || {
                                    popup
                                        .options
                                        .get()
                                        .into_iter()
                                        .enumerate()
                                        .collect::<Vec<_>>()
                                },
                                |(index, option): &(usize, SearchSuggestion)| {
                                    format!("{}:{index}", option.word)
                                },
                                move |entry, _| {
                                    let option =
                                        computed(move || entry.with(|(_, option)| option.clone()));
                                    view()
                                        .on_layout(move |bounds: &Bounds<Points>| {
                                            scroll_state
                                                .row(entry.with(|(index, _)| *index), bounds);
                                        })
                                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| {
                                            popup.selected.set(entry.with(|(index, _)| *index));
                                        })
                                        .child(
                                            suggestion(option)
                                                .selected(move || {
                                                    entry.with(|(index, _)| *index)
                                                        == popup.selected.get()
                                                })
                                                .on_press(move || {
                                                    pick(entry.with(|(index, _)| *index))
                                                }),
                                        )
                                },
                            )),
                    )
            })),
    )
}

fn suggestion(option: Memo<SearchSuggestion>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .role(Some(nsh_core::AccessibilityRole::Option))
        .accessibility_label(move || Some(option.with(|option| option.word.clone())))
        .w_full()
        .min_h(move || px(if is_desktop() { 34.0 } else { 48.0 }))
        .px_2p5()
        .py(px(7.0))
        .flex_row()
        .items_center()
        .justify_start()
        .gap_2p5()
        .rounded(move || theme().radius)
        .text_color(move || theme().popover_foreground)
        .hover(|style| style.bg(move || theme().accent.opacity(0.6)))
        .selected_style(|style| style.bg(move || theme().accent))
        .active(|style| style.bg(move || theme().accent).opacity(0.8))
        .transition_colors()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                motion::FAST_MS
            }
        })
        .child(
            icon(Icon::Search)
                .size_3p5()
                .text_color(move || theme().muted_foreground),
        )
        .child(when(
            move || option.with(|option| !option.cover.is_empty()),
            move || {
                image(move || ImageSrc::from(option.with(|option| option.cover.clone())))
                    .w_10()
                    .h(px(54.0))
                    .flex_shrink_0()
                    .rounded(radii::control())
                    .bg(move || theme().muted)
                    .object_fit(nsh_core::ResizeMode::Cover)
                    .accessible(false)
            },
        ))
        .child(
            view()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    text("")
                        .text_sm()
                        .number_of_lines(1)
                        .font_weight(FontWeight::NORMAL)
                        .style_when(
                            move || option.with(|option| option.rich),
                            |style| style.font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(dynamic(move || {
                            let option = option.get();
                            let parts = if option.parts.is_empty() {
                                vec![(option.word, false)]
                            } else {
                                option.parts
                            };
                            text("").children(parts.into_iter().map(|(label, highlighted)| {
                                text(label).text_color(move || {
                                    if highlighted {
                                        theme().color("warm").expect("warm token")
                                    } else {
                                        theme().popover_foreground
                                    }
                                })
                            }))
                        })),
                )
                .child(when(
                    move || option.with(|option| !option.description.is_empty()),
                    move || {
                        text(move || option.with(|option| option.description.clone()))
                            .text_xs()
                            .number_of_lines(1)
                            .text_color(move || theme().muted_foreground)
                    },
                )),
        )
}
