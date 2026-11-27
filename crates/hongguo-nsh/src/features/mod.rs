pub(crate) mod browse;
pub(crate) mod catalog;
pub(crate) mod creator;
pub(crate) mod detail;
pub(crate) use detail::DetailPrefill;
pub(crate) mod downloads;
pub(crate) mod emoji;
mod highlight_album;
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
        Route::AlbumPlayer(id, entry) => highlight_album::page(session, nav, id, entry),
        Route::Player(id, episode) => crate::features::player::page(session, nav, id, episode),
        Route::History => section_page(route, session, nav),
        Route::Settings => settings::page(session, nav),
        Route::Account => settings::account(session, nav),
        Route::Tasks
        | Route::Merge
        | Route::Storage
        | Route::Rank
        | Route::New
        | Route::Browse
        | Route::Follows
        | Route::Collections
        | Route::Liked
        | Route::Reservations => section_page(route, session, nav),
    }
}

fn section_page(route: Route, session: SessionHandle, nav: Navigation) -> Screen {
    let root = route.section_root();
    let title = match root {
        Route::History => "nav.my",
        Route::Browse => "nav.browse.title",
        _ => "nav.primaryDownloads",
    };
    crate::shell::page(nav, move || crate::i18n::tr(title))
        .header_visible(is_desktop())
        .child(crate::shell::section_header(nav))
        .child(view().flex_1().min_h_0().child(dynamic(move || {
            let current = nav.routes().with(|routes| routes.last().cloned());
            match current.filter(|route| route.section_root() == root) {
                Some(Route::History) => history::page(session, nav),
                Some(Route::Collections) => library::page(session, nav, library::Kind::Collections),
                Some(Route::Liked) => library::page(session, nav, library::Kind::Liked),
                Some(Route::Follows) => library::page(session, nav, library::Kind::Follows),
                Some(Route::Reservations) => {
                    library::page(session, nav, library::Kind::Reservations)
                }
                Some(Route::Browse) => browse::page(session, nav),
                Some(Route::Rank) => rank::page(session, nav),
                Some(Route::New) => new_drama::page(session, nav),
                Some(Route::Tasks) => downloads::page(session, nav),
                Some(Route::Merge) => maintenance::merge::page(session, nav),
                Some(Route::Storage) => maintenance::storage::page(session, nav),
                _ => view(),
            }
        })))
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
