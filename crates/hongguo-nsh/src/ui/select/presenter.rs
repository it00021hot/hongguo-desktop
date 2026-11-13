use super::SelectOption;
use crate::ui::{Icon, icon, radii};
use nsh_base::{PopoverEvent, SheetEvent, theme};
use nsh_core::prelude::*;
mod options;

#[derive(Debug, Clone, PartialEq, Props)]
pub(crate) struct ChoiceSelectProps {
    label: Prop<SharedString>,
    items: Prop<Vec<SelectOption>>,
    value: Prop<Option<SharedString>>,
    #[prop(default)]
    disabled: Prop<bool>,
    trigger: Prop<Option<Slot>>,
}

#[derive(Debug, Clone, PartialEq, Event)]
pub(crate) enum ChoiceSelectEvent {
    Change(Option<SharedString>),
}

#[component]
pub(crate) fn ChoiceSelect(
    props: ChoiceSelectProps,
    cx: Scope<ChoiceSelectEvent>,
) -> impl IntoElement {
    let open = signal(false);
    let focused = signal(false);
    let restore = stored(true);
    let reference = element_ref();
    let width = signal(0.0_f32);
    let items = computed(move || props.items.get());
    let value = computed(move || props.value.get());
    let label = props.label;
    let disabled = props.disabled;
    effect(move || {
        if disabled.get() {
            open.set(false);
            return;
        }
        if focused.get() && !open.get() {
            for key in ["up", "down"] {
                on_key(key, move |_| open.set(true));
            }
        }
    });
    let close = move || open.set(false);
    let choose = move |next: SharedString| {
        if value.get_untracked().as_ref() != Some(&next) {
            cx.emit(ChoiceSelectEvent::Change(Some(next)));
        }
        restore.replace(true);
        close();
    };
    let body = move || options::body(items, value, choose, close);
    let focus_back = move || {
        if restore.get() && is_desktop() {
            after_answer(move || reference.focus());
        }
    };
    let trigger = move || {
        view()
            .element_ref(&reference)
            .role(nsh_core::AccessibilityRole::Combobox)
            .accessible(true)
            .label(move || label.get())
            .disabled(move || disabled.get())
            .accessibility_state(move || AccessibilityState {
                expanded: Some(open.get()),
                disabled: disabled.get(),
                ..Default::default()
            })
            .focusable(move || !disabled.get())
            .on_focus(move || focused.set(true))
            .on_blur(move || focused.set(false))
            .on_press(move || {
                if !disabled.get_untracked() {
                    restore.replace(true);
                    open.update(|open| *open = !*open);
                }
            })
            .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
            .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
            .child(if props.trigger.get_untracked().is_some() {
                props.trigger.place(|trigger| trigger)
            } else {
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
                    .px_3()
                    .py_1()
                    .rounded(radii::control)
                    .border_1()
                    .border_color(move || theme().input)
                    .bg(move || theme().card)
                    .text_color(move || theme().foreground)
                    .style_when(move || disabled.get(), |style| style.opacity(0.45))
                    .hover(|style| style.bg(move || theme().accent))
                    .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
                    .child(
                        text(move || {
                            let current = value.get();
                            items
                                .get()
                                .into_iter()
                                .find(|item| Some(&item.value) == current.as_ref())
                                .map(|item| item.title)
                                .unwrap_or_default()
                        })
                        .flex_1()
                        .text_sm(),
                    )
                    .child(
                        icon(Icon::Dropdown)
                            .size_4()
                            .text_color(move || theme().muted_foreground),
                    )
                    .into_element()
            })
    };
    // The retained inner trigger owns keyboard focus; Slot may bind its own reference to the wrapper.
    if is_desktop() {
        nsh_base::popover()
            .open(move || Some(open.get()))
            .placement(Placement::Bottom)
            .align(Align::Start)
            .offset(px(4.0))
            .trigger(view().child(trigger()))
            .on_open_change(move |event: &PopoverEvent| {
                if let PopoverEvent::OpenChange(next, reason) = event {
                    restore.replace(*reason != nsh_base::OpenChangeReason::PressOutside);
                    if !disabled.get_untracked() || !next {
                        open.set(*next);
                    }
                }
            })
            .on_close(focus_back)
            .popup_style(move |style| {
                style
                    .min_w(move || px(width.get()))
                    .p_1()
                    .rounded(radii::control)
                    .border_1()
                    .border_color(move || theme().border)
                    .bg(move || theme().popover)
                    .text_color(move || theme().popover_foreground)
                    .shadow_md()
            })
            .child(dynamic(body))
            .into_element()
    } else {
        view()
            .child(trigger())
            .child(
                nsh_base::sheet()
                    .open(move || Some(open.get()))
                    .side(Placement::Bottom)
                    .on_open_change(move |event: &SheetEvent| {
                        if let SheetEvent::OpenChange(next, _) = event {
                            open.set(*next);
                        }
                    })
                    .on_close(focus_back)
                    .backdrop_style(|style| style.bg(move || theme().background.opacity(0.8)))
                    .popup_style(|style| {
                        style
                            .w_full()
                            .p_4()
                            .pb_safe_area_plus(px(16.0))
                            .rounded_t(radii::control)
                            .bg(move || theme().popover)
                            .text_color(move || theme().popover_foreground)
                    })
                    .child(dynamic(body)),
            )
            .into_element()
    }
}
