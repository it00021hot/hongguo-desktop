use super::{actions::Reservations, view};
use crate::navigation::{DetailPrefill, Navigation, Route};
use hongguo_api::calendar::CalendarPage;
use nsh_core::prelude::*;
use std::collections::HashSet;

pub(super) fn content(nav: Navigation) -> View {
    let mut page: CalendarPage =
        serde_json::from_str(include_str!("../../../../preview/fixtures/calendar.json"))
            .expect("calendar preview data");
    for item in &mut page.items {
        item.cover = crate::pages::fixtures::FRAME_PATH.into();
    }
    let today = page.default_date.clone();
    let page = stored(page);
    let selected = crate::view_memory::scope("new").signal("date", String::new());
    let feed = resource(
        move || Some(selected.get()),
        move |date, _| {
            let mut page = page.get();
            if !date.is_empty() && date != page.default_date {
                page.items.clear();
            }
            std::future::ready(Ok(page))
        },
    );
    let reservations = Reservations {
        reserved: signal(HashSet::new()),
        pending: signal(HashSet::new()),
        message: signal(SharedString::default()),
    };
    view::presentation(
        view::State {
            feed,
            selected,
            reservations,
            today,
        },
        view::Events {
            open: Box::new(move |item| nav.detail(DetailPrefill::from_calendar(&item))),
            play: Box::new(move |item| nav.push(Route::Player(item.series_id, 1))),
            reserve: Box::new(move |item| {
                reservations.reserved.update(|reserved| {
                    reserved.insert(item.series_id);
                })
            }),
            more: Box::new(|| {}),
        },
    )
}
