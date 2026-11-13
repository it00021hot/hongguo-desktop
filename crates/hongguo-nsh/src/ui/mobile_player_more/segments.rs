use super::{MobilePlayerMoreAction, MobilePlayerMoreState, ink};
use crate::i18n::tr;
use nsh_core::prelude::*;

const RATE_MATCH_TOLERANCE: f32 = 0.01;
const DEFAULT_RATE: f32 = 1.0;

pub(super) fn rates(
    state: MobilePlayerMoreState,
    action: impl Fn(MobilePlayerMoreAction) + Copy + 'static,
) -> View {
    view()
        .gap_2()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(heading(move || tr("player.playbackRate")))
                .child(view().flex_1())
                .child(
                    text(move || format!("{}x", state.rate.get()))
                        .font_family("monospace")
                        .text_xs()
                        .text_color(move || ink("video-controls").opacity(0.5)),
                ),
        )
        .child(
            nsh_base::radio_group().child(
                more_choices()
                    .labels(
                        state
                            .rates
                            .iter()
                            .map(|value| format!("{value}x").into())
                            .collect::<Vec<SharedString>>(),
                    )
                    .selected(move || {
                        state
                            .rates
                            .iter()
                            .position(|value| {
                                (*value - state.rate.get()).abs() < RATE_MATCH_TOLERANCE
                            })
                            .or_else(|| state.rates.iter().position(|value| *value == DEFAULT_RATE))
                            .map(|index| index as u32)
                    })
                    .columns(3_u16)
                    .busy(state.busy)
                    .on_change(move |index: &u32| {
                        if let Some(rate) = state.rates.get(*index as usize) {
                            action(MobilePlayerMoreAction::SetRate(*rate));
                        }
                    }),
            ),
        )
}

pub(super) fn definitions(
    state: MobilePlayerMoreState,
    action: impl Fn(MobilePlayerMoreAction) + Copy + 'static,
) -> View {
    view()
        .gap_2()
        .child(heading(move || tr("player.definition")))
        .child(when(
            move || state.definitions.with(Vec::is_empty),
            move || {
                text(move || tr("player.definitionLocalOnly"))
                    .text_xs()
                    .text_color(move || ink("video-controls").opacity(0.45))
            },
        ))
        .child(when(
            move || !state.definitions.with(Vec::is_empty),
            move || {
                nsh_base::radio_group().child(
                    more_choices()
                        .labels(move || {
                            std::iter::once(tr("player.definitionAuto"))
                                .chain(
                                    state
                                        .definitions
                                        .get()
                                        .iter()
                                        .map(|definition| format!("{}P", definition.value).into()),
                                )
                                .collect::<Vec<SharedString>>()
                        })
                        .selected(move || match state.selected_definition.get() {
                            None => Some(0),
                            Some(value) => state
                                .definitions
                                .with(|definitions| {
                                    definitions
                                        .iter()
                                        .position(|definition| definition.value == value)
                                })
                                .map(|index| index as u32 + 1),
                        })
                        .columns(move || {
                            (((window().width.0 - 64.0 + 6.0) / 78.0).floor().max(1.0) as usize)
                                .min(state.definitions.with(Vec::len) + 1)
                                as u16
                        })
                        .busy(state.busy)
                        .on_change(move |index: &u32| {
                            if *index == 0 {
                                action(MobilePlayerMoreAction::SetDefinition(None));
                            } else if let Some(value) = state.definitions.with(|definitions| {
                                definitions
                                    .get(*index as usize - 1)
                                    .map(|definition| definition.value)
                            }) {
                                action(MobilePlayerMoreAction::SetDefinition(Some(value)));
                            }
                        }),
                )
            },
        ))
}

fn heading(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_sm()
        .line_height(px(20.0))
        .text_color(move || ink("video-controls").opacity(0.85))
}

#[derive(Debug, Clone, PartialEq, Props)]
struct MoreChoicesProps {
    labels: Prop<Vec<SharedString>>,
    selected: Prop<Option<u32>>,
    columns: Prop<u16>,
    busy: Prop<bool>,
}

#[derive(Debug, Clone, PartialEq, Event)]
enum MoreChoicesEvent {
    Change(u32),
}

#[component]
fn MoreChoices(props: MoreChoicesProps, cx: Scope<MoreChoicesEvent>) -> impl IntoElement {
    view().grid().grid_cols(props.columns).gap_1p5().child(each(
        move || props.labels.get(),
        Clone::clone,
        move |label, index| {
            let node = element_ref();
            nsh_base::radio()
                .element_ref(&node)
                .accessibility_label(move || Some(label.get()))
                .checked(move || props.selected.get() == Some(index.get() as u32))
                .disabled(props.busy)
                .on_change(move |_: &bool| {
                    cx.emit(MoreChoicesEvent::Change(index.get_untracked() as u32));
                    after_answer(move || node.focus());
                })
                .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                .px_2()
                .py_2()
                .rounded(move || nsh_base::theme().radius)
                .items_center()
                .justify_center()
                .bg(move || ink("video-controls").opacity(0.05))
                .text_color(move || ink("video-controls").opacity(0.7))
                .checked_style(|style| {
                    style
                        .bg(move || ink("video-controls").opacity(0.2))
                        .text_color(move || ink("video-controls"))
                })
                .hover(move |style| {
                    style.bg(move || {
                        ink("video-controls").opacity(
                            if props.selected.get() == Some(index.get() as u32) {
                                0.2
                            } else {
                                0.1
                            },
                        )
                    })
                })
                .active(|style| style.opacity(0.8))
                .focus(|style| {
                    style
                        .outline(px(2.0))
                        .outline_color(move || ink("video-controls"))
                })
                .disabled_style(|style| style.opacity(0.4))
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { super::FAST_MS })
                .child(
                    text(move || label.get())
                        .font_family("monospace")
                        .text_xs()
                        .line_height(px(16.0)),
                )
        },
    ))
}
