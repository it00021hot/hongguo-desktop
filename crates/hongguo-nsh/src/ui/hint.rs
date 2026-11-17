use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn hint(
    label: impl IntoReactive<SharedString>,
    placement: Placement,
) -> nsh_base::TooltipElement {
    nsh_base::tooltip()
        .placement(placement)
        .px_2()
        .py_1()
        .rounded(crate::ui::radii::control)
        .bg(move || theme().popover)
        .text_color(move || theme().popover_foreground)
        .child(text(label).text_xs())
}
