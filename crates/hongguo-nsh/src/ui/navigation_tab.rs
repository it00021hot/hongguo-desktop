use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn navigation_tab(
    label: impl IntoReactive<SharedString>,
    selected: Memo<bool>,
) -> nsh_base::ButtonElement {
    super::button(label, super::ButtonVariant::Quiet)
        .rounded(px(0.0))
        .border_b(px(2.0))
        .border_color(move || {
            if selected.get() {
                theme().primary
            } else {
                theme().background.opacity(0.0)
            }
        })
        .selected(selected)
        .selected_style(super::palette::selected_label)
        .hover(super::palette::highlighted_label)
}

pub(crate) fn profile_action(label: impl IntoReactive<SharedString>) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    if !is_desktop() {
        return nsh_base::button()
            .accessibility_label(move || label.get())
            .min_h_12()
            .min_w(px(80.0))
            .items_center()
            .justify_center()
            .disabled_style(|style| style.opacity(0.45))
            .active(|style| style.opacity(0.8))
            .focus_visible(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
            .child(
                view()
                    .h_8()
                    .px_4()
                    .items_center()
                    .justify_center()
                    .rounded(super::radii::control())
                    .bg(move || theme().primary)
                    .text_color(move || theme().primary_foreground)
                    .child(text(label).text_sm().font_weight(FontWeight::MEDIUM)),
            );
    }
    super::button(label, super::ButtonVariant::Primary)
}
