mod calendar;
mod model;
pub(crate) mod presentation;
mod recommend;
mod upcoming;

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
    Upcoming,
}

pub(super) fn page(session: SessionHandle, nav: Navigation) -> View {
    let mode = controls();
    let reservations = calendar::actions::Reservations::observe(session);
    let active = |value| {
        computed(move || {
            mode.get() == value
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::New))
        })
    };
    presentation(
        nav,
        mode,
        [
            (
                Mode::Recommend,
                recommend::content(session, nav, active(Mode::Recommend)),
            ),
            (
                Mode::Calendar,
                calendar::content(session, nav, active(Mode::Calendar), reservations),
            ),
            (
                Mode::Upcoming,
                upcoming::content(session, nav, active(Mode::Upcoming), reservations),
            ),
        ],
    )
}

fn controls() -> Signal<Mode> {
    crate::view_memory::cached("new-controls", || {
        crate::view_memory::scope("new").signal("mode", Mode::Recommend)
    })
}

fn presentation(nav: Navigation, mode: Signal<Mode>, panels: [(Mode, View); 3]) -> View {
    ui::toolbar_slot(
        move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::New))
        },
        move || mode_tabs(mode),
    );
    crate::shell::content(nav, move || tr("nav.new.title")).child(
        view()
            .flex_1()
            .min_h(px(0.0))
            .gap_3()
            .pt_3()
            .child(when(move || !is_desktop(), move || mode_tabs(mode).px_4()))
            .children(panels.map(|(value, panel)| {
                panel.style_when(move || mode.get() != value, |style| style.hidden())
            })),
    )
}

fn mode_tabs(mode: Signal<Mode>) -> View {
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_1()
        .children(
            [
                (Mode::Recommend, "newDrama.tabs.recommend"),
                (Mode::Calendar, "newDrama.tabs.calendar"),
                (Mode::Upcoming, "newDrama.tabs.upcoming"),
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
