pub(crate) mod browse;
pub(crate) mod catalog;
pub(crate) mod creator;
pub(crate) mod detail;
pub(crate) use detail::DetailPrefill;
pub(crate) mod downloads;
pub(crate) mod emoji;
pub(crate) mod history;
pub(crate) mod library;
pub(crate) mod maintenance;
pub(crate) mod new_drama;
pub(crate) mod player;
pub(crate) mod rank;
mod series_feed;
pub(crate) mod settings;
pub(crate) use settings::account_dialogs;

pub(crate) use crate::events::SessionHandle;
use crate::navigation::{Navigation, Route};
use nsh_core::prelude::*;

pub(crate) fn render(route: Route, session: SessionHandle, nav: Navigation) -> Screen {
    match route {
        Route::Catalog => catalog::page(session, nav),
        Route::Detail(id) => detail::page(session, nav, id),
        Route::DetailWithPrefill(prefill) => detail::seeded_page(session, nav, prefill),
        Route::Creator(id) => creator::page(session, nav, id),
        Route::Player(id, episode) => crate::features::player::page(session, nav, id, episode),
        Route::History => history::page(session, nav),
        Route::Settings => settings::page(session, nav),
        Route::Account => settings::account(session, nav),
        Route::Tasks => downloads::page(session, nav),
        Route::Merge => maintenance::merge::page(session, nav),
        Route::Storage => maintenance::storage::page(session, nav),
        Route::Rank => rank::page(session, nav),
        Route::New => new_drama::page(session, nav),
        Route::Browse => browse::page(session, nav),
        Route::Follows => {
            crate::features::library::page(session, nav, crate::features::library::Kind::Follows)
        }
        Route::Collections => crate::features::library::page(
            session,
            nav,
            crate::features::library::Kind::Collections,
        ),
        Route::Liked => {
            crate::features::library::page(session, nav, crate::features::library::Kind::Liked)
        }
        Route::Reservations => crate::features::library::page(
            session,
            nav,
            crate::features::library::Kind::Reservations,
        ),
    }
}

pub(crate) fn error_state(
    error: impl IntoReactive<SharedString>,
    retry: impl Fn() + 'static,
) -> impl IntoElement {
    crate::ui::status(error).child(
        crate::ui::button(
            move || crate::i18n::tr("common.retry"),
            crate::ui::ButtonVariant::Secondary,
        )
        .on_press(retry),
    )
}
