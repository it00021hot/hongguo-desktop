mod browse;
mod catalog;
mod creator;
mod detail;
pub(crate) use detail::DetailPrefill;
mod fixtures;
mod library;
mod maintenance;
mod new_drama;
mod rank;
mod series_feed;
mod settings;
pub(crate) use settings::{account_dialogs, preview_account_dialogs};

pub(crate) use crate::events::SessionHandle;
use crate::navigation::{Navigation, Route};
use nsh_core::prelude::*;
pub(crate) fn preview_new(nav: Navigation) -> Screen {
    new_drama::preview(nav)
}
pub(crate) fn preview_rank(nav: Navigation) -> Screen {
    rank::preview(nav)
}
pub(crate) fn preview_browse(nav: Navigation) -> Screen {
    browse::preview(nav)
}
pub(crate) fn preview_creator(nav: Navigation, user_id: String) -> Screen {
    creator::preview(nav, user_id)
}
pub(crate) fn preview_creators() -> Vec<hongguo_api::detail::SeriesCreator> {
    creator::preview_creators()
}
pub(crate) fn preview_history(nav: Navigation, cover: nsh_core::element::ImageSrc) -> Screen {
    library::preview_history(nav, cover)
}
pub(crate) fn preview_tasks(nav: Navigation) -> Screen {
    library::preview_tasks(nav)
}
pub(crate) fn preview_merge(nav: Navigation) -> Screen {
    maintenance::merge::preview(nav)
}
pub(crate) fn preview_storage(nav: Navigation) -> Screen {
    maintenance::storage::preview(nav)
}

pub(crate) fn preview_settings(nav: Navigation) -> Screen {
    settings::preview(nav)
}
pub(crate) fn preview_detail(
    nav: Navigation,
    series: hongguo_common::model::Series,
    meta: hongguo_api::detail::SeriesMeta,
) -> Screen {
    detail::preview(nav, series, meta)
}
pub(crate) fn preview_upcoming(nav: Navigation, prefill: DetailPrefill) -> Screen {
    detail::preview_upcoming(nav, prefill)
}

pub(crate) fn render(route: Route, session: SessionHandle, nav: Navigation) -> Screen {
    match route {
        Route::Catalog => catalog::page(session, nav),
        Route::Detail(id) => detail::page(session, nav, id),
        Route::DetailWithPrefill(prefill) => detail::seeded_page(session, nav, prefill),
        Route::Creator(id) => creator::page(session, nav, id),
        Route::Player(id, episode) => crate::player::page(session, nav, id, episode),
        Route::History => library::history(session, nav),
        Route::Settings => settings::page(session, nav),
        Route::Account => settings::account(session, nav),
        Route::Tasks => library::tasks(session, nav),
        Route::Merge => maintenance::merge::page(session, nav),
        Route::Storage => maintenance::storage::page(session, nav),
        Route::Rank => rank::page(session, nav),
        Route::New => new_drama::page(session, nav),
        Route::Browse => browse::page(session, nav),
        Route::Follows => crate::library::page(session, nav, crate::library::Kind::Follows),
        Route::Collections => crate::library::page(session, nav, crate::library::Kind::Collections),
        Route::Liked => crate::library::page(session, nav, crate::library::Kind::Liked),
        Route::Reservations => {
            crate::library::page(session, nav, crate::library::Kind::Reservations)
        }
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
