use super::{model::Query, search::Search};
use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct GlobalSearch {
    draft: Signal<String>,
    submitted: Signal<(u64, String)>,
}

impl GlobalSearch {
    pub(crate) fn provide() {
        provide_context(Self {
            draft: crate::view_memory::scope("browse").signal("draft", String::new()),
            submitted: signal((0, String::new())),
        });
    }

    pub(super) fn connect(query: Signal<Query>) {
        let search = use_context::<Self>().expect("global search owner");
        effect(move || {
            let (revision, term) = search.submitted.get();
            if revision > 0 {
                query.update(|query| {
                    query.term = term;
                    query.reset_paging();
                });
            }
        });
    }
}

pub(crate) fn toolbar(session: SessionHandle, nav: Navigation) -> View {
    let state = use_context::<GlobalSearch>().expect("global search owner");
    let search = Search::new(
        state.draft,
        move || is_desktop().then(|| session.identity()),
        move |term| {
            let app = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_service::catalog_service::suggestions(&term, &app).await
            })
        },
    );
    let submit = move || {
        let term = search.term();
        if term.is_empty() {
            return;
        }
        search.dismiss();
        state.submitted.update(|(revision, query)| {
            *revision += 1;
            *query = term;
        });
        nav.push(Route::Browse);
    };
    view()
        .window_drag(false)
        .relative()
        .w_52()
        .child(super::search::field(search, submit, move |item| {
            if item.cover.is_empty() || item.series_id.is_empty() {
                search.change(&item.word);
                submit();
            } else {
                search.dismiss();
                nav.push(Route::Detail(item.series_id));
            }
        }))
        .child(
            ui::button("", ui::ButtonVariant::Quiet)
                .accessibility_label(move || tr("search.submit"))
                .absolute()
                .left_0()
                .top_0()
                .min_h_8()
                .size_8()
                .p_0()
                .child(ui::icon(ui::Icon::Search))
                .disabled(move || search.term().is_empty())
                .on_press(submit),
        )
}
