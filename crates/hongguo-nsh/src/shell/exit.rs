use crate::{i18n::tr, ui};
use nsh_core::{desktop::WindowAttributes, element::desktop, prelude::*};
use std::{cell::Cell, rc::Rc};

pub(super) fn confirmation(requested: Signal<bool>) -> impl IntoElement {
    let window = desktop::current_window();
    let mut attributes = WindowAttributes::default();
    attributes.close_on_press = Some(false);
    window.set(attributes);
    window.on_close_press(move |_| requested.set(true));
    desktop::on_quit_request(move || requested.set(true));

    let confirmed = Rc::new(Cell::new(false));
    let cancel_check = confirmed.clone();
    let was_open = Cell::new(false);
    effect(move || {
        let open = requested.get();
        if was_open.replace(open) && !open && !cancel_check.get() {
            desktop::cancel_quit_request();
        }
    });

    ui::confirmation(
        requested,
        move || tr("window.exitTitle"),
        move || tr("window.exitConfirm"),
        || text(move || tr("window.exitBody")).text_sm(),
        move || {
            confirmed.set(true);
            desktop::quit();
        },
    )
}
