use crate::{i18n::tr, ui};
use nsh_core::{desktop::WindowAttributes, element::desktop, prelude::*};

pub(super) fn confirmation(requested: Signal<bool>) -> impl IntoElement {
    let window = desktop::current_window();
    let mut attributes = WindowAttributes::default();
    attributes.close_on_press = Some(false);
    window.set(attributes);
    window.on_close_press(move |_| requested.set(true));

    ui::confirmation(
        requested,
        move || tr("window.exitTitle"),
        move || tr("window.exitConfirm"),
        || text(move || tr("window.exitBody")).text_sm(),
        desktop::quit,
    )
}
