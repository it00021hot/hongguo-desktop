mod segments;
#[cfg(test)]
mod tests;

use super::{DanmakuSettingsState, Icon, danmaku_settings_compact, icon, motion::FAST_MS};
use crate::i18n::tr;
use hongguo_common::model::VideoDefinition;
use nsh_base::{SheetEvent, theme};
use nsh_core::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(crate) struct MobilePlayerMoreState {
    pub open: Memo<bool>,
    pub rate: Signal<f32>,
    pub rates: &'static [f32],
    pub definitions: Memo<Vec<VideoDefinition>>,
    pub selected_definition: Memo<Option<u32>>,
    pub danmaku_on: Signal<bool>,
    pub danmaku: DanmakuSettingsState,
    pub busy: Memo<bool>,
    pub episode_count: Memo<u32>,
    pub enhancement_available: Memo<bool>,
}

#[derive(Clone, Debug)]
pub(crate) enum MobilePlayerMoreAction {
    SetRate(f32),
    SetDefinition(Option<u32>),
    ToggleDanmaku,
    Episodes,
    Download,
    Enhancement,
}

pub(crate) struct MobilePlayerMoreEvents {
    pub change: Box<dyn Fn(bool)>,
    pub action: Box<dyn Fn(MobilePlayerMoreAction)>,
}

pub(crate) fn mobile_player_more(
    state: MobilePlayerMoreState,
    events: MobilePlayerMoreEvents,
    trigger: nsh_base::ButtonElement,
    trigger_ref: ElementRef,
) -> View {
    let events = stored(Rc::new(events));
    let expanded = signal(false);
    effect(move || {
        if !state.open.get() {
            expanded.set(false);
            return;
        }
        for key in ["left", "right", "up", "down"] {
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::NotEditing,
                    repeat: true,
                    ..Default::default()
                },
                |_| {},
            );
        }
    });
    let change = move |open| {
        let events = events.with(|events| events.clone());
        (events.change)(open);
    };
    let action = move |action| {
        let events = events.with(|events| events.clone());
        (events.action)(action);
    };
    view()
        .flex_shrink_0()
        .child(
            trigger
                .element_ref(&trigger_ref)
                .disabled(state.busy)
                .on_press(move || change(true)),
        )
        .child(
            nsh_base::sheet()
                .open(move || Some(state.open.get()))
                .side(Placement::Bottom)
                .close_on_swipe(true)
                .on_open_change(move |event: &SheetEvent| {
                    if let SheetEvent::OpenChange(next, _) = event {
                        change(*next);
                    }
                })
                .on_close(move || trigger_ref.focus())
                .backdrop_style(|style| style.bg(move || ink("video-surface").opacity(0.5)))
                .popup_style(|style| {
                    style
                        .max_h(move || px(window().height.0 * 0.78))
                        .rounded_t(px(16.0))
                        .border_t_1()
                        .border_color(move || ink("video-controls").opacity(0.1))
                        .bg(move || ink("video-sheet"))
                        .text_color(move || ink("video-controls"))
                        .pt_2()
                })
                .child(dynamic(move || {
                    view()
                        .relative()
                        .child(
                            scroll()
                                .max_h(move || px(window().height.0 * 0.78 - 8.0))
                                .child(
                                    view()
                                        .gap_5()
                                        .pl_4()
                                        .pr_12()
                                        .pt_2()
                                        .pb_safe_area_plus(px(16.0))
                                        .test_id("mobile-player-more")
                                        .role(AccessibilityRole::Group)
                                        .label(move || tr("player.playbackRate"))
                                        .child(segments::rates(state, action))
                                        .child(segments::definitions(state, action))
                                        .child(actions(state, expanded, change, action)),
                                ),
                        )
                        .child(
                            close_button()
                                .absolute()
                                .top_2()
                                .right_4()
                                .on_press(move || change(false)),
                        )
                })),
        )
}

fn actions(
    state: MobilePlayerMoreState,
    expanded: Signal<bool>,
    change: impl Fn(bool) + Copy + 'static,
    action: impl Fn(MobilePlayerMoreAction) + Copy + 'static,
) -> View {
    view()
        .rounded(super::radii::card())
        .overflow_hidden()
        .bg(move || ink("video-controls").opacity(0.05))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px_3()
                .h(move || px(if is_desktop() { 44.0 } else { 48.0 }))
                .child(
                    text(move || tr("player.danmaku"))
                        .text_sm()
                        .line_height(px(14.0))
                        .flex_1(),
                )
                .child(
                    more_switch(state.danmaku_on)
                        .disabled(state.busy)
                        .on_change(move |_: &bool| action(MobilePlayerMoreAction::ToggleDanmaku)),
                ),
        )
        .child(separator())
        .child(
            row(move || tr("player.danmakuSettings"), None)
                .disabled(state.busy)
                .on_press(move || expanded.update(|value| *value = !*value))
                .child(chevron(move || expanded.get())),
        )
        .child(when(expanded, move || {
            view().p_3().child(danmaku_settings_compact(state.danmaku))
        }))
        .child(separator())
        .child(
            row(move || tr("player.enhanceSettings"), Some(Icon::New))
                .disabled(move || !state.enhancement_available.get() || state.busy.get())
                .on_press(move || {
                    change(false);
                    action(MobilePlayerMoreAction::Enhancement);
                })
                .child(chevron(|| false)),
        )
        .child(separator())
        .child(
            row(
                move || {
                    let count = state.episode_count.get();
                    if count > 0 {
                        tr("player.episodesBar")
                            .replace("{count}", &count.to_string())
                            .into()
                    } else {
                        tr("player.episodes")
                    }
                },
                Some(Icon::Episodes),
            )
            .disabled(move || state.busy.get() || state.episode_count.get() == 0)
            .on_press(move || {
                change(false);
                action(MobilePlayerMoreAction::Episodes);
            })
            .child(chevron(|| false)),
        )
        .child(separator())
        .child(
            row(move || tr("player.downloadToLocal"), Some(Icon::Download))
                .disabled(move || state.busy.get() || state.episode_count.get() == 0)
                .on_press(move || {
                    change(false);
                    action(MobilePlayerMoreAction::Download);
                })
                .child(chevron(|| false)),
        )
}

fn row(label: impl IntoReactive<SharedString>, glyph: Option<Icon>) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    nsh_base::button()
        .accessibility_label(move || Some(label.get()))
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_3()
        .min_h(move || px(if is_desktop() { 44.0 } else { 48.0 }))
        .hover(|style| style.bg(move || ink("video-controls").opacity(0.05)))
        .active(|style| style.opacity(0.8))
        .focus(|style| {
            style
                .outline(px(2.0))
                .outline_color(move || ink("video-controls"))
        })
        .disabled_style(|style| style.opacity(0.4))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(when(
            move || glyph.is_some(),
            move || icon(glyph.expect("present glyph")).size_4().opacity(0.7),
        ))
        .child(
            text(move || label.get())
                .text_sm()
                .line_height(px(20.0))
                .flex_1()
                .number_of_lines(1),
        )
}

fn more_switch(checked: Signal<bool>) -> nsh_base::SwitchElement {
    nsh_base::switch()
        .accessibility_label(move || tr("player.danmaku"))
        .checked(checked)
        .hit_slop(if is_desktop() { 4.0 } else { 14.0 })
        .track_style(|style| {
            style
                .w_8()
                .h(px(18.4))
                .border_1()
                .border_color(move || ink("video-controls").opacity(0.0))
                .justify_center()
                .rounded_full()
                .bg(move || theme().input)
                .checked(|style| style.bg(move || theme().primary))
        })
        .thumb_style(|style| {
            style
                .size_4()
                .rounded_full()
                .bg(move || theme().background)
                .checked(|style| style.translate_x(px(14.0)))
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        })
        .focus(|style| {
            style
                .outline(px(2.0))
                .outline_color(move || ink("video-controls"))
        })
        .disabled_style(|style| style.opacity(0.4))
}

fn chevron(expanded: impl IntoReactive<bool>) -> Svg {
    let expanded = expanded.into_reactive();
    icon(Icon::Dropdown)
        .size_4()
        .opacity(0.4)
        .rotate(move || deg(if expanded.get() { 0.0 } else { -90.0 }))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}

fn separator() -> View {
    view()
        .h(px(1.0))
        .bg(move || ink("video-controls").opacity(0.1))
}

fn close_button() -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || Some(tr("common.close")))
        .size_4()
        .min_h(px(0.0))
        .p_0()
        .hit_slop(if is_desktop() { 8.0 } else { 16.0 })
        .rounded(px(2.0))
        .opacity(0.8)
        .hover(|style| style.opacity(1.0))
        .focus(|style| {
            style
                .outline(px(2.0))
                .outline_color(move || ink("video-controls"))
        })
        .child(icon(Icon::Close).size_4())
}

fn ink(name: &str) -> AdaptiveColor {
    theme().color(name).expect("mobile player semantic token")
}
