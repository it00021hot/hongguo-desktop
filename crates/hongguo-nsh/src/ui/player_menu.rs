use super::{Icon, icon, motion::FAST_MS, radii};
use nsh_base::{MenuEvent, PopoverEvent, SheetEvent, theme};
use nsh_core::prelude::*;
mod navigation;

pub(crate) fn player_menu(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> nsh_base::MenuElement {
    let open = open.into_reactive();
    let open = computed(move || open.get());
    let change = stored(Box::new(change) as Box<dyn Fn(bool)>);
    let trigger_ref = element_ref();
    nsh_base::menu()
        .open(move || Some(open.get()))
        .placement(Placement::Top)
        .align(Align::End)
        .offset(px(4.0))
        .margin(px(8.0))
        .trigger(retained_trigger(trigger, trigger_ref, open, change))
        .on_open_change(move |event: &MenuEvent| {
            if let MenuEvent::OpenChange(open, _) = event {
                change.with(|change| change(*open));
            }
        })
        .on_close(move || trigger_ref.focus())
        .popup_style(|style| {
            style
                .min_w(px(128.0))
                .p_1()
                .rounded(radii::control())
                .border_1()
                .border_color(move || theme().border)
                .bg(move || theme().popover)
                .text_color(move || theme().popover_foreground)
                .shadow_md()
                .origin_bottom_right()
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                .ease_out()
        })
        .popup_starting_style(|style| {
            style.opacity(0.0).scale(move || {
                if reduce_motion() {
                    1.0
                } else {
                    super::motion::PRESS_SCALE
                }
            })
        })
        .popup_ending_style(|style| {
            style.opacity(0.0).scale(move || {
                if reduce_motion() {
                    1.0
                } else {
                    super::motion::PRESS_SCALE
                }
            })
        })
        .backdrop_style(|style| style.bg(move || theme().background.opacity(0.6)))
        .child(dynamic(move || {
            navigation::body(open, move || change.with(|change| change(false)), &content)
        }))
}

pub(crate) fn player_menu_option(
    label: impl IntoReactive<SharedString>,
    checked: impl IntoReactive<bool>,
) -> nsh_base::MenuItemElement {
    let checked = checked.into_reactive();
    let checked = computed(move || checked.get());
    let label = label.into_reactive();
    let node = element_ref();
    if let Some(navigation) = use_context::<navigation::Navigation>() {
        navigation.register(node, checked);
    }
    nsh_base::menu_item()
        .element_ref(&node)
        .accessibility_label(move || Some(label.get()))
        .role(Some(nsh_core::AccessibilityRole::MenuItemRadio))
        .checked(move || Some(checked.get()))
        .min_h(px(if is_desktop() { 32.0 } else { 48.0 }))
        .px_2()
        .pr_6()
        .py_1p5()
        .relative()
        .rounded(px(4.0))
        .flex_row()
        .items_center()
        .gap_2()
        .text_color(move || theme().popover_foreground)
        .hover(|style| style.bg(move || theme().accent))
        .focus(|style| {
            style
                .bg(move || theme().accent)
                .text_color(move || theme().accent_foreground)
        })
        .disabled_style(|style| style.opacity(0.45))
        .child(
            view()
                .absolute()
                .right_2()
                .w_3()
                .child(when(move || checked.get(), || icon(Icon::Check).size_3())),
        )
}

pub(crate) fn player_menu_label(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_sm()
        .line_height(px(20.0))
        .font_weight(FontWeight::MEDIUM)
        .px_2()
        .py_1p5()
}

/// A freeform player panel: an anchored desktop popup, a native mobile sheet.
pub(crate) fn player_panel(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    title: impl IntoReactive<SharedString>,
    width: f32,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> View {
    panel(
        open,
        change,
        title,
        width,
        trigger,
        content,
        PanelKind::Ordinary,
    )
}

pub(crate) fn player_danmaku_panel(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> View {
    panel(
        open,
        change,
        move || crate::i18n::tr("player.danmakuSettings"),
        240.0,
        trigger,
        content,
        PanelKind::Danmaku,
    )
}

pub(crate) fn player_enhancement_panel(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> View {
    panel(
        open,
        change,
        move || crate::i18n::tr("player.enhanceSettings"),
        240.0,
        trigger,
        content,
        PanelKind::Danmaku,
    )
}

#[derive(Clone, Copy)]
enum PanelKind {
    Ordinary,
    Episodes,
    Danmaku,
}

pub(crate) fn player_episode_panel(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> View {
    panel(
        open,
        change,
        move || crate::i18n::tr("player.episodes"),
        460.0,
        trigger,
        content,
        PanelKind::Episodes,
    )
}

pub(crate) fn player_episode_sheet(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    return_focus: ElementRef,
    content: impl Fn() -> View + 'static,
) -> View {
    let open = open.into_reactive();
    let open = computed(move || open.get());
    let change = stored(Box::new(change) as Box<dyn Fn(bool)>);
    bottom_panel(
        open,
        change,
        computed(move || crate::i18n::tr("player.episodes")),
        return_focus,
        content,
    )
}

fn panel(
    open: impl IntoReactive<bool>,
    change: impl Fn(bool) + 'static,
    title: impl IntoReactive<SharedString>,
    width: f32,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
    kind: PanelKind,
) -> View {
    let episodes = matches!(kind, PanelKind::Episodes);
    let danmaku = matches!(kind, PanelKind::Danmaku);
    let open = open.into_reactive();
    let open = computed(move || open.get());
    effect(move || {
        if !open.get() {
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
    let change = stored(Box::new(change) as Box<dyn Fn(bool)>);
    let title = title.into_reactive();
    let title = computed(move || title.get());
    let trigger_ref = element_ref();
    if is_desktop() {
        return view().flex_shrink_0().child(
            nsh_base::popover()
                .open(move || Some(open.get()))
                .placement(Placement::Top)
                .align(Align::End)
                .offset(px(if episodes || danmaku { 12.0 } else { 8.0 }))
                .margin(px(8.0))
                .trigger(retained_trigger(trigger, trigger_ref, open, change))
                .on_open_change(move |event: &PopoverEvent| {
                    if let PopoverEvent::OpenChange(open, _) = event {
                        change.with(|change| change(*open));
                    }
                })
                .on_close(move || trigger_ref.focus())
                .popup_style(move |style| {
                    style
                        .w(px(width))
                        .max_w(move || px(window().width.0 * 0.92))
                        .max_h(move || {
                            px(if episodes {
                                window().height.0 * 0.62
                            } else {
                                (window().height.0 - 96.0).max(96.0)
                            })
                        })
                        .p_4()
                        .rounded(if danmaku { px(12.0) } else { radii::card() })
                        .border(px(if episodes { 0.0 } else { 1.0 }))
                        .border_color(move || video_ink().opacity(0.1))
                        .bg(move || {
                            if episodes {
                                theme().color("video-picker").expect("video picker token")
                            } else {
                                video_surface().opacity(if danmaku { 0.85 } else { 0.95 })
                            }
                        })
                        .text_color(video_ink)
                        .origin_bottom_right()
                        .transition()
                        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                        .ease_out()
                })
                .popup_starting_style(|style| {
                    style.opacity(0.0).scale(move || {
                        if reduce_motion() {
                            1.0
                        } else {
                            super::motion::PRESS_SCALE
                        }
                    })
                })
                .popup_ending_style(|style| {
                    style.opacity(0.0).scale(move || {
                        if reduce_motion() {
                            1.0
                        } else {
                            super::motion::PRESS_SCALE
                        }
                    })
                })
                .child(dynamic(move || focus_panel(content(), title))),
        );
    }
    view()
        .flex_shrink_0()
        .child(
            trigger
                .element_ref(&trigger_ref)
                .on_press(move || change.with(|change| change(true))),
        )
        .child(bottom_panel(open, change, title, trigger_ref, content))
}

fn bottom_panel(
    open: Memo<bool>,
    change: Stored<Box<dyn Fn(bool)>>,
    title: Memo<SharedString>,
    return_focus: ElementRef,
    content: impl Fn() -> View + 'static,
) -> View {
    view().flex_shrink_0().child(
        nsh_base::sheet()
            .open(move || Some(open.get()))
            .side(Placement::Bottom)
            .close_on_swipe(true)
            .on_open_change(move |event: &SheetEvent| {
                if let SheetEvent::OpenChange(open, _) = event {
                    change.with(|change| change(*open));
                }
            })
            .on_close(move || return_focus.focus())
            .backdrop_style(|style| style.bg(move || video_surface().opacity(0.65)))
            .popup_style(|style| {
                style
                    .w(move || window().width)
                    .h(move || px(window().height.0 * 0.72))
                    .rounded(radii::card())
                    .bg(video_surface)
                    .text_color(video_ink)
            })
            .child(dynamic(move || {
                view()
                    .flex_1()
                    .gap_3()
                    .p_4()
                    .pb_safe_area_plus(px(16.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                text(move || title.get())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                super::player_icon_button(Icon::Close, move || {
                                    crate::i18n::tr("common.close")
                                })
                                .on_press(move || change.with(|change| change(false))),
                            ),
                    )
                    .child(focus_panel(content(), title))
            })),
    )
}

fn focus_panel(content: View, title: Memo<SharedString>) -> View {
    let node = element_ref();
    let focused = stored(false);
    view()
        .element_ref(&node)
        .focusable(true)
        .role(nsh_core::AccessibilityRole::Group)
        .label(move || title.get())
        .on_layout(move |_: &Bounds<Points>| {
            if !focused.replace(true) {
                node.focus();
            }
        })
        .child(content)
}

// The base popup installs its own anchor ref on this wrapper; keep the
// actual button's ref separate so closing can restore keyboard focus.
fn retained_trigger(
    trigger: nsh_base::ButtonElement,
    reference: ElementRef,
    open: Memo<bool>,
    change: Stored<Box<dyn Fn(bool)>>,
) -> View {
    view()
        .focusable(false)
        .pointer_events(nsh_core::PointerEvents::BoxNone)
        .child(
            trigger
                .element_ref(&reference)
                .on_press(move || change.with(|change| change(!open.get_untracked()))),
        )
}

pub(crate) fn player_episode(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
) -> nsh_base::RadioElement {
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let label = label.into_reactive();
    nsh_base::radio()
        .accessibility_label(move || Some(label.get()))
        .checked(selected)
        .h(px(if is_desktop() { 20.0 } else { 48.0 }))
        .min_w(move || px(if is_desktop() { 0.0 } else { 48.0 }))
        .items_center()
        .justify_center()
        .text_color(move || {
            theme()
                .color("video-picker-muted")
                .expect("video picker muted token")
        })
        .font_weight(FontWeight::NORMAL)
        .checked_style(|style| {
            style
                .text_color(video_ink)
                .font_weight(FontWeight::SEMIBOLD)
        })
        .hover(|style| {
            style.text_color(move || {
                theme()
                    .color("video-picker-text")
                    .expect("video picker text token")
            })
        })
        .focus(|style| style.outline(px(2.0)).outline_color(video_ink))
        .disabled_style(|style| style.opacity(0.45))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}

pub(crate) fn player_volume_slider(value: impl IntoReactive<f32>) -> nsh_base::SliderElement {
    let value = value.into_reactive();
    nsh_base::slider()
        .accessibility_label(move || Some(crate::i18n::tr("player.volume")))
        .axis(nsh_core::geometry::Axis::Vertical)
        .min(0.0)
        .max(1.0)
        .step(0.01)
        .value(move || Some(nsh_base::SliderValue::from(value.get() as f64)))
        .h(px(96.0))
        .min_w(px(32.0))
        .track_style(|style| {
            style
                .w(px(if is_desktop() { 32.0 } else { 48.0 }))
                .h(px(96.0))
                .items_center()
        })
        .indicator_style(|style| {
            style
                .w_1()
                .h_full()
                .rounded_full()
                .bg(move || video_ink().opacity(0.25))
        })
        .range_style(|style| {
            style
                .w_1()
                .rounded_full()
                .bg(move || video_ink().opacity(0.9))
        })
        .thumb_style(|style| style.size_3().rounded_full().bg(video_ink))
        .thumb_class("hover:opacity-90 active:opacity-80 focus:outline-2 focus:outline-ring")
}

pub(crate) fn player_episode_group() -> nsh_base::RadioGroupElement {
    nsh_base::radio_group()
}

pub(crate) fn player_episode_cell(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let label = label.into_reactive();
    let node = element_ref();
    let initial = stored(true);
    effect(move || {
        let selected = selected.get();
        if !initial.replace(false) && selected {
            node.focus();
        }
    });
    nsh_base::button()
        .element_ref(&node)
        .accessibility_label(move || Some(label.get()))
        .selected(selected)
        .font_weight(FontWeight::NORMAL)
        .h(px(if is_desktop() { 36.0 } else { 48.0 }))
        .min_w(px(0.0))
        .items_center()
        .justify_center()
        .rounded(radii::control())
        .bg(move || {
            theme()
                .color("video-picker-cell")
                .expect("video picker cell token")
        })
        .text_color(move || {
            theme()
                .color("video-picker-text")
                .expect("video picker text token")
        })
        .selected_style(|style| {
            style
                .text_color(video_ink)
                .font_weight(FontWeight::SEMIBOLD)
                .bg(move || {
                    theme()
                        .color("video-picker-active")
                        .expect("video selected token")
                })
        })
        .hover(move |style| {
            style.bg(move || {
                if selected.get() {
                    theme()
                        .color("video-picker-active")
                        .expect("video picker active token")
                } else {
                    theme()
                        .color("video-picker-hover")
                        .expect("video picker hover token")
                }
            })
        })
        .focus(|style| style.outline(px(2.0)).outline_color(video_ink))
        .disabled_style(|style| style.opacity(0.45))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
}

pub(crate) fn player_toggle_icon(
    off: Icon,
    on: Icon,
    active: impl IntoReactive<bool>,
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    let active = active.into_reactive();
    let active = computed(move || active.get());
    let label = label.into_reactive();
    let label = computed(move || label.get());
    super::video::player_surface()
        .accessibility_label(move || Some(label.get()))
        .size(px(if is_desktop() { 32.0 } else { 48.0 }))
        .p_0()
        .child(super::player_hint(move || label.get()))
        .child(
            view()
                .relative()
                .size_4()
                .child(
                    icon(off)
                        .absolute()
                        .inset_0()
                        .size_4()
                        .opacity(move || if active.get() { 0.0_f32 } else { 1.0_f32 })
                        .scale(move || {
                            if active.get() && !reduce_motion() {
                                0.8_f32
                            } else {
                                1.0_f32
                            }
                        })
                        .transition()
                        .duration(move || {
                            if reduce_motion() {
                                0.0
                            } else {
                                super::motion::CHROME_MS
                            }
                        }),
                )
                .child(
                    icon(on)
                        .absolute()
                        .inset_0()
                        .size_4()
                        .opacity(move || if active.get() { 1.0_f32 } else { 0.0_f32 })
                        .scale(move || {
                            if !active.get() && !reduce_motion() {
                                0.8_f32
                            } else {
                                1.0_f32
                            }
                        })
                        .transition()
                        .duration(move || {
                            if reduce_motion() {
                                0.0
                            } else {
                                super::motion::CHROME_MS
                            }
                        }),
                ),
        )
}

fn video_ink() -> nsh_core::element::AdaptiveColor {
    theme()
        .color("video-controls")
        .expect("video foreground token")
}
fn video_surface() -> nsh_core::element::AdaptiveColor {
    theme().color("video-surface").expect("video surface token")
}
