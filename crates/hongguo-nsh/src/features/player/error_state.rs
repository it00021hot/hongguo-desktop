use crate::{bridge::Failure, i18n::tr, ui};
use nsh_core::prelude::*;

fn may_retry(error: Option<&Failure>) -> bool {
    error.is_none_or(Failure::can_retry)
}

pub(super) fn retry(error: Option<&Failure>, action: impl FnOnce()) {
    if may_retry(error) {
        action();
    }
}

pub(super) fn panel(
    error: Memo<Option<Failure>>,
    fallback: Signal<SharedString>,
    on_retry: impl Fn() + Copy + 'static,
) -> View {
    ui::status(move || {
        error
            .get()
            .map_or_else(|| fallback.get(), |error| error.text())
    })
    .child(when(
        move || error.with(|error| may_retry(error.as_ref())),
        move || {
            ui::button(move || tr("common.retry"), ui::ButtonVariant::Secondary).on_press(on_retry)
        },
    ))
}
