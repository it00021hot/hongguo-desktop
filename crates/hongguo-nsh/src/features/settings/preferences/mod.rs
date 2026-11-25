mod cache;
mod controls;
mod directory;
mod feedback;
mod keyboard;
mod model;
mod proxy;
mod runtime;
mod sections;
mod transcode;
mod updates;

use crate::{events::SessionHandle, i18n::tr, navigation::Navigation, ui};
use model::Draft;
use nsh_core::prelude::*;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    let mut initial = Draft::from(session.with(|session| session.state.settings()));
    if initial.cache_dir.is_empty() {
        initial.cache_dir = hongguo_service::settings_service::default_cache_dir();
    }
    let draft = signal(initial);
    let busy = signal(false);
    let message = signal(SharedString::default());
    updates::install(session, draft, message);
    let runtime = runtime::Runtime::production(session, nav, message);
    let active = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&crate::navigation::Route::Settings))
    });
    crate::shell::page(nav, move || tr("nav.settings.title")).child(
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(when(
                move || !message.get().is_empty(),
                move || {
                    view()
                        .w_full()
                        .max_w(px(768.0))
                        .self_center()
                        .px_6()
                        .pt_3()
                        .child(ui::status(message))
                },
            ))
            .child(ui::form(
                view()
                    .w_full()
                    .max_w(px(768.0))
                    .self_center()
                    .p_6()
                    .gap_4()
                    .child(sections::download(draft, busy, message))
                    .child(sections::naming(draft, busy))
                    .child(proxy::section(draft, busy, message))
                    .child(crate::features::settings::presentation::form_separator())
                    .child(sections::playback(draft, busy, transcode::section(runtime)))
                    .child(when(is_desktop, move || keyboard::shortcuts(active)))
                    .child(keyboard::boss(draft, busy))
                    .child(cache::section(draft, busy, message, runtime))
                    .child(crate::update::card())
                    .child(feedback::section()),
            )),
    )
}
