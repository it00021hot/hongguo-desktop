use super::Kind;
use crate::{
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

pub(super) fn frame(
    kind: Kind,
    toolbar: View,
    logged_in: Memo<bool>,
    nav: Navigation,
    body: impl Fn() -> View + 'static,
) -> View {
    view()
        .flex_1()
        .gap(move || {
            px(if matches!(kind, Kind::Collections | Kind::Liked) {
                12.0
            } else {
                16.0
            })
        })
        .p(move || px(if kind == Kind::Follows { 0.0 } else { 16.0 }))
        .child(toolbar)
        .child(when(
            move || kind == Kind::Reservations && !logged_in.get(),
            move || {
                ui::personal_login_required(
                    "reservation.loginRequired",
                    ui::Icon::Reservations,
                    move || nav.push(Route::Account),
                )
            },
        ))
        .child(when(logged_in, body))
}

#[cfg(test)]
mod tests;
