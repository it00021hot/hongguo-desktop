mod calendar;
#[cfg(test)]
mod lifetime_tests;
mod model;
mod recommend;

use super::SessionHandle;
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Recommend,
    Calendar,
}

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    let (mode, recommend_active, calendar_active) = controls(nav);
    presentation(
        nav,
        mode,
        recommend::content(session, nav, recommend_active),
        calendar::content(session, nav, calendar_active),
    )
}

fn controls(nav: Navigation) -> (Signal<Mode>, Memo<bool>, Memo<bool>) {
    crate::view_memory::cached("new-controls", move || {
        let mode = crate::view_memory::scope("new").signal("mode", Mode::Recommend);
        let recommend_active = computed(move || {
            mode.get() == Mode::Recommend
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::New))
        });
        let calendar_active = computed(move || {
            mode.get() == Mode::Calendar
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::New))
        });
        (mode, recommend_active, calendar_active)
    })
}

pub(super) fn preview(nav: Navigation) -> Screen {
    let memory = crate::view_memory::scope("new");
    let mode = memory.signal("mode", Mode::Recommend);
    let audience = memory.signal("audience", model::Audience::All);
    let mut items = super::fixtures::catalog().rank.items;
    for item in &mut items {
        item.cover = super::fixtures::FRAME_PATH.into();
    }
    let items = stored(items);
    let cards = items
        .get()
        .into_iter()
        .enumerate()
        .map(|(index, item)| model::card(item, usize::from(index == 0)))
        .collect::<Vec<_>>();
    let grid = ui::new_drama_grid(
        signal(cards),
        true,
        move |id| {
            if let Some(item) =
                items.with(|items| items.iter().find(|item| item.series_id == id).cloned())
            {
                nav.detail(crate::navigation::DetailPrefill::from_rank(&item));
            }
        },
        || {},
        move || audience.get().value(),
    );
    presentation(
        nav,
        mode,
        recommend::presentation(audience, move |value| audience.set(value), grid),
        calendar::preview(nav),
    )
}

fn presentation(nav: Navigation, mode: Signal<Mode>, recommend: View, calendar: View) -> Screen {
    ui::toolbar_slot(
        move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::New))
        },
        move || mode_tabs(mode),
    );
    ui::page(nav, move || tr("nav.new.title")).child(
        view()
            .flex_1()
            .min_h(px(0.0))
            .gap_3()
            .px_4()
            .pt_3()
            .child(when(move || !is_desktop(), move || mode_tabs(mode)))
            .child(recommend.style_when(
                move || mode.get() != Mode::Recommend,
                |style| style.hidden(),
            ))
            .child(
                calendar.style_when(move || mode.get() != Mode::Calendar, |style| style.hidden()),
            ),
    )
}

fn mode_tabs(mode: Signal<Mode>) -> View {
    view().flex_row().items_center().gap_1().children(
        [
            (Mode::Recommend, "newDrama.tabs.recommend"),
            (Mode::Calendar, "newDrama.tabs.calendar"),
        ]
        .into_iter()
        .map(move |(value, label)| {
            let tab = if is_desktop() {
                ui::header_tab(move || tr(label))
            } else {
                ui::button(move || tr(label), ui::ButtonVariant::Secondary)
            };
            tab.selected(move || mode.get() == value)
                .on_press(move || mode.set(value))
        }),
    )
}
