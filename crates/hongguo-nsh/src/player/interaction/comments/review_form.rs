use crate::{emoji, i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) struct Events {
    pub send: Box<dyn Fn()>,
    pub login: Box<dyn Fn()>,
}

pub(super) fn form(
    editor: emoji::Editor,
    stars: Signal<u8>,
    pending: Signal<bool>,
    logged_in: Memo<bool>,
    events: Events,
) -> View {
    let events = stored(events);
    let rating = ui::ReviewFocus(std::array::from_fn(|_| element_ref()));
    let next = element_ref();
    ui::review_card()
        .child(ui::review_rating(stars, pending, rating))
        .child(when(
            move || !logged_in.get(),
            move || {
                ui::button(
                    move || tr("settings.loginAction"),
                    ui::ButtonVariant::Secondary,
                )
                .on_press(move || events.with(|events| (events.login)()))
            },
        ))
        .child(emoji::sendbox(
            editor,
            pending,
            logged_in,
            None,
            emoji::comment_picker(editor, next),
            emoji::SendEvents {
                send: Box::new(move || events.with(|events| (events.send)())),
                pick_image: Box::new(|| {}),
            },
            emoji::SendOptions {
                allowed: computed(move || super::review_model::review_score(stars.get()).is_some()),
                review: true,
                previous: Box::new(move || {
                    rating.0[usize::from(stars.get_untracked().clamp(1, 5)) - 1].focus()
                }),
                next,
            },
        ))
}
