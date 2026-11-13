use super::{ButtonVariant, Icon, icon, motion::FAST_MS, radii};
use nsh_base::{PopoverEvent, theme};
use nsh_core::prelude::*;

const MAX_WINDOW_RATIO: f32 = 0.6;
const PANEL_PADDING: f32 = 12.0;
const PANEL_BORDER: f32 = 1.0;

pub(crate) fn rank_filter_scroll() -> Scroll {
    // Bound the scroll owner itself; an ancestor's max-height does not
    // constrain a content-sized Scroll. Account for the panel's insets.
    scroll().min_h(px(0.0)).flex_shrink(1.0).max_h(move || {
        px((window().height.0 * MAX_WINDOW_RATIO - 2.0 * (PANEL_PADDING + PANEL_BORDER)).max(0.0))
    })
}

pub(crate) fn rank_filter_panel(
    open: Signal<bool>,
    trigger: nsh_base::ButtonElement,
    content: impl Fn() -> View + 'static,
) -> View {
    let reference = element_ref();
    let content = stored(Box::new(content) as Box<dyn Fn() -> View>);
    if !is_desktop() {
        return view()
            .child(
                trigger
                    .element_ref(&reference)
                    .on_press(move || open.set(true)),
            )
            .child(super::sheet(
                open,
                move || crate::i18n::tr("rank.filter.title"),
                move || content.with(|render| render()),
            ));
    }
    view().child(
        nsh_base::popover()
            .open(move || Some(open.get()))
            .placement(Placement::Bottom)
            .align(Align::End)
            .offset(px(4.0))
            .margin(px(0.0))
            .trigger(
                view().child(
                    trigger
                        .element_ref(&reference)
                        .on_press(move || open.update(|open| *open = !*open)),
                ),
            )
            .on_open_change(move |event: &PopoverEvent| {
                if let PopoverEvent::OpenChange(value, _) = event {
                    open.set(*value);
                }
            })
            .on_close(move || reference.focus())
            .popup_style(|style| {
                style
                    .w(px(384.0))
                    .max_h(move || px(window().height.0 * MAX_WINDOW_RATIO))
                    .p(px(PANEL_PADDING))
                    .rounded(radii::control())
                    .border(px(PANEL_BORDER))
                    .border_color(move || theme().border)
                    .bg(move || theme().popover)
                    .text_color(move || theme().popover_foreground)
                    .shadow_md()
                    .transition()
                    .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                    .ease_out()
            })
            .popup_starting_style(|style| style.opacity(0.0))
            .popup_ending_style(|style| style.opacity(0.0))
            .child(dynamic(move || content.with(|render| render()))),
    )
}

pub(crate) fn rank_filter_close() -> nsh_base::ButtonElement {
    super::controls::surface(ButtonVariant::Quiet)
        .accessibility_label(move || crate::i18n::tr("common.close"))
        .size_4()
        .min_h(px(16.0))
        .p_0()
        .child(icon(Icon::Close))
}

pub(crate) fn rank_filter_option(
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    super::controls::surface(ButtonVariant::Quiet)
        .min_h(px(if is_desktop() { 22.0 } else { 48.0 }))
        .px_2p5()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(move || theme().border)
        .selected_style(|style| {
            style
                .border_color(move || theme().primary)
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
        })
        .child(
            text(label)
                .text_xs()
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL),
        )
}
