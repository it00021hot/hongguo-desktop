mod task_actions;
mod task_confirm;
mod task_row;
mod tasks;

use super::SessionHandle;
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;
use tasks::TaskIntent;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> View {
    let files = crate::file_access::FileAccess::current();
    let state = session.with(|session| session.state.clone());
    let result = resource(
        move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::Tasks))
                .then(|| session.downloads())
        },
        move |_, _| {
            let state = state.clone();
            bridge::request(async move { Ok((state.queue().all(), state.queue().status())) })
        },
    );
    let busy = signal(false);
    let feedback = signal(SharedString::default());
    let dispatch = move |intent: TaskIntent| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        feedback.set(SharedString::default());
        let state = session.with(|session| session.state.clone());
        spawn(async move {
            match task_actions::run(state, intent, files).await {
                Ok(notice) => feedback.set(notice),
                Err(error) => feedback.set(error.text()),
            }
            busy.set(false);
            result.refetch();
        });
    };
    crate::shell::content(nav, move || tr("nav.tasks.title")).child(
        view()
            .flex_1()
            .child(when(
                move || result.loading() && result.with(|data| data.is_none()),
                queue_loading,
            ))
            .child(ui::status(feedback))
            .child(when(
                move || result.error().is_some(),
                move || {
                    super::error_state(
                        move || {
                            result
                                .error()
                                .map_or_else(SharedString::default, |error| error.text())
                        },
                        move || result.refetch(),
                    )
                },
            ))
            .child(when(
                move || result.with(|data| data.is_some()),
                move || {
                    tasks::content(
                        move || {
                            result.with(|data| {
                                data.map(|(items, _)| items.clone()).unwrap_or_default()
                            })
                        },
                        move || {
                            result.with(|data| {
                                data.map(|(_, status)| status.clone()).unwrap_or_default()
                            })
                        },
                        busy,
                        dispatch,
                        move || nav.push(Route::Browse),
                    )
                },
            )),
    )
}

fn queue_loading() -> View {
    use ui::loading::skeleton;

    view()
        .role(AccessibilityRole::Group)
        .label(move || tr("common.loading"))
        .gap_2()
        .child(skeleton().h(px(34.0)).w(percentage(0.52)))
        .child(
            view()
                .gap_2()
                .child(skeleton().h(px(36.0)).w_full())
                .children((0..4).map(|_| skeleton().h(px(53.0)).w_full())),
        )
}
