use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn skeleton() -> View {
    view()
        .bg(move || theme().accent)
        .rounded(crate::ui::radii::control())
        .animate_pulse()
        .style_when(reduce_motion, |style| style.animate_none())
        .accessible(false)
}
