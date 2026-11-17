use super::{Navigation, Route};
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) fn primary(route: Route) -> Route {
    match route {
        Route::Rank | Route::New => Route::Browse,
        Route::Merge | Route::Storage => Route::Tasks,
        Route::Collections
        | Route::Liked
        | Route::Follows
        | Route::Reservations
        | Route::Account => Route::History,
        other => other,
    }
}

pub(crate) fn header(nav: Navigation) -> View {
    let current = move || nav.routes().with(|routes| routes.last().cloned());
    let tabs = view().flex_shrink_0().children([
        group(
            nav,
            Route::History,
            vec![
                ("nav.history.title", Route::History),
                ("nav.collections.title", Route::Collections),
                ("nav.liked.title", Route::Liked),
                ("nav.follows.title", Route::Follows),
                ("nav.reservations.title", Route::Reservations),
            ],
        ),
        group(
            nav,
            Route::Browse,
            vec![
                ("nav.browse.title", Route::Browse),
                ("nav.rank.title", Route::Rank),
                ("nav.new.title", Route::New),
            ],
        ),
        group(
            nav,
            Route::Tasks,
            vec![
                ("nav.tasks.title", Route::Tasks),
                ("nav.merge.title", Route::Merge),
                ("nav.storage.title", Route::Storage),
            ],
        ),
    ]);
    view()
        .flex_shrink_0()
        .child(when(
            move || current().is_some_and(|route| primary(route) == Route::History),
            move || {
                view()
                    .px_4()
                    .py_4()
                    .gap_4()
                    .style_when(
                        move || !is_desktop(),
                        |style| style.px_5().pt_0().pb_2().gap_2(),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .child(
                                text(move || tr("nav.my"))
                                    .text_2xl()
                                    .style_when(
                                        move || !is_desktop(),
                                        |style| style.text_size(px(22.0)),
                                    )
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(view().flex_1())
                            .child(when(
                                move || !is_desktop(),
                                super::mobile_preferences::Preferences::theme_button,
                            ))
                            .child(
                                ui::button(
                                    move || {
                                        if is_desktop() {
                                            tr("nav.settings.title")
                                        } else {
                                            SharedString::default()
                                        }
                                    },
                                    ui::ButtonVariant::Quiet,
                                )
                                .accessibility_label(move || tr("nav.settings.title"))
                                .style_when(move || !is_desktop(), |style| style.size_12().p_0())
                                .child(when(move || !is_desktop(), || ui::icon(ui::Icon::Settings)))
                                .on_press(move || nav.push(Route::Settings)),
                            ),
                    )
                    .child(crate::features::settings::profile())
            },
        ))
        .child(when(
            move || !is_desktop() && current().is_some_and(|route| primary(route) == Route::Tasks),
            move || {
                view().h_12().px_5().justify_center().child(
                    text(move || tr("nav.primaryDownloads"))
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD),
                )
            },
        ))
        .child(tabs)
}

fn group(nav: Navigation, section: Route, tabs: Vec<(&'static str, Route)>) -> View {
    view().child(when(
        move || {
            nav.routes()
                .with(|routes| routes.last().cloned())
                .is_some_and(|route| primary(route) == section)
        },
        move || {
            scroll().horizontal().flex_shrink_0().child(
                view()
                    .flex_row()
                    .gap_1()
                    .px_4()
                    .py_2()
                    .style_when(move || !is_desktop(), |style| style.py_0())
                    .children(
                        tabs.iter()
                            .map(|(label, route)| {
                                let label = *label;
                                let selected = route.clone();
                                let destination = route.clone();
                                let active = computed(move || {
                                    nav.routes().with(|routes| routes.last() == Some(&selected))
                                });
                                ui::navigation_tab(move || tr(label), active)
                                    .min_h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
                                    .on_press(move || nav.push(destination.clone()))
                            })
                            .collect::<Vec<_>>(),
                    ),
            )
        },
    ))
}
