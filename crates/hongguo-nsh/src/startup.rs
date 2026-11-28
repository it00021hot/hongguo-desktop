use crate::{
    events::SessionHandle,
    navigation::{Navigation, Route},
};
use hongguo_common::model::settings::{StartupMode, StartupPage};
use nsh_core::{prelude::*, storage::Storage};

const LAST_PAGE_KEY: &str = "guava.lastPage";
// Transient routes are deliberately excluded; never persist a media ID or reopen playback.
const PAGES: &[(&str, Route)] = &[
    ("home", Route::Catalog),
    ("browse", Route::Browse),
    ("rank", Route::Rank),
    ("new", Route::New),
    ("downloads", Route::Tasks),
    ("merge", Route::Merge),
    ("storage", Route::Storage),
    ("history", Route::History),
    ("collections", Route::Collections),
    ("liked", Route::Liked),
    ("follows", Route::Follows),
    ("reservations", Route::Reservations),
];

pub(crate) fn navigation(session: SessionHandle) -> Navigation {
    let settings = session.with(|session| session.state.settings());
    let saved = Storage::get_item(LAST_PAGE_KEY);
    let nav = Navigation::new(initial_route(
        settings.startup_mode,
        settings.startup_page,
        saved.as_deref(),
    ));
    let saved = stored(saved);
    effect(move || {
        let key = nav.routes().with(|routes| routes.last().and_then(page_key));
        if let Some(key) = key
            && !saved.with(|saved| saved.as_deref() == Some(key))
        {
            match Storage::set_item(LAST_PAGE_KEY, key) {
                Ok(()) => {
                    saved.replace(Some(key.into()));
                }
                Err(error) => log::warn!("[NSH startup] saving last page failed: {error}"),
            }
        }
    });
    nav
}

fn page_key(route: &Route) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(_, page)| page == route)
        .map(|(key, _)| *key)
}

fn initial_route(mode: StartupMode, fixed: StartupPage, last_page: Option<&str>) -> Route {
    if mode == StartupMode::LastPage {
        return PAGES
            .iter()
            .find(|(key, _)| Some(*key) == last_page)
            .map_or(Route::Browse, |(_, route)| route.clone());
    }
    match fixed {
        StartupPage::Home => Route::Catalog,
        StartupPage::Browse => Route::Browse,
        StartupPage::Downloads => Route::Tasks,
        StartupPage::My => Route::History,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_page_ignores_last_page() {
        for (page, route) in [
            (StartupPage::Home, Route::Catalog),
            (StartupPage::Browse, Route::Browse),
            (StartupPage::Downloads, Route::Tasks),
            (StartupPage::My, Route::History),
        ] {
            assert_eq!(initial_route(StartupMode::Fixed, page, Some("new")), route);
        }
    }

    #[test]
    fn restores_stable_tabs_without_a_playback_fallback() {
        for (key, route) in PAGES {
            assert_eq!(page_key(route), Some(*key));
            assert_eq!(
                initial_route(StartupMode::LastPage, StartupPage::Home, Some(key)),
                *route
            );
        }
        for route in [
            Route::Settings,
            Route::Account,
            Route::Detail("42".into()),
            Route::Player("42".into(), 2),
        ] {
            assert_eq!(page_key(&route), None);
        }
        for value in [None, Some("unknown"), Some("player")] {
            assert_eq!(
                initial_route(StartupMode::LastPage, StartupPage::Home, value),
                Route::Browse
            );
        }
    }
}
