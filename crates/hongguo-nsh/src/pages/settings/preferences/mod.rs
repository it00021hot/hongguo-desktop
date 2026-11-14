mod cache;
mod controls;
mod directory;
mod keyboard;
mod model;
mod proxy;
mod runtime;
mod sections;
mod transcode;

use crate::{bridge, events::SessionHandle, i18n::tr, navigation::Navigation, ui};
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
    let runtime = runtime::Runtime::production(session, nav, message);
    ui::page(nav, move || tr("nav.settings.title")).child(form(
        draft,
        busy,
        message,
        runtime,
        computed(move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&crate::navigation::Route::Settings))
        }),
        true,
        move || save(session, draft, busy, message),
    ))
}

pub(super) fn preview(nav: Navigation) -> Screen {
    let latest = signal(hongguo_common::model::Settings {
        download_dir: "/Users/milo/Movies/Hongguo".into(),
        max_concurrency: 3,
        proxy: hongguo_common::model::ProxyConfig {
            mode: hongguo_common::model::settings::ProxyMode::System,
            url: String::new(),
        },
        cache_limit_mb: 4096,
        cache_ttl_days: 7,
        cache_dir: "/Users/milo/Library/Caches/hongguo-downloader".into(),
        ..Default::default()
    });
    let draft = signal(Draft::from(latest.get_untracked()));
    let busy = signal(false);
    let message = signal(SharedString::default());
    let runtime = runtime::Runtime::preview(nav, message);
    ui::page(nav, move || tr("nav.settings.title")).child(form(
        draft,
        busy,
        message,
        runtime,
        computed(move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&crate::navigation::Route::Settings))
        }),
        false,
        move || {
            if let Some(keys) = crate::shortcuts::Shortcuts::current() {
                keys.cancel_capture();
            }
            match validate(draft.get_untracked(), latest.get_untracked(), is_desktop()) {
                Ok(saved) => {
                    latest.set(saved.clone());
                    draft.set(Draft::from(saved));
                    message.set(tr("settings.saved"));
                }
                Err(error) => message.set(error),
            }
        },
    ))
}

fn form(
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
    runtime: runtime::Runtime,
    active: Memo<bool>,
    external: bool,
    save_action: impl Fn() + 'static,
) -> View {
    let tasks = super::Tasks::provide();
    ui::form(
        view()
            .w_full()
            .max_w(px(768.0))
            .self_center()
            .p_6()
            .gap_4()
            .child(super::account::card())
            .child(sections::download(draft, busy, message, external))
            .child(sections::naming(draft, busy))
            .child(proxy::section(draft, busy, message, external))
            .child(ui::form_separator())
            .child(sections::playback(draft, busy, transcode::section(runtime)))
            .child(when(is_desktop, move || keyboard::shortcuts(active)))
            .child(keyboard::boss(draft, busy))
            .child(cache::section(draft, busy, message, external, runtime))
            .child(crate::update::card(external))
            .child(
                ui::form_save(move || tr("settings.save"))
                    .self_end()
                    .disabled(move || busy.get() || tasks.pending.get())
                    .on_press(save_action),
            )
            .child(ui::status(message)),
    )
    .test_id("preferences-form")
}

fn validate(
    draft: Draft,
    latest: hongguo_common::model::Settings,
    desktop: bool,
) -> Result<hongguo_common::model::Settings, SharedString> {
    let settings = draft.apply(latest, desktop).map_err(|error| {
        SharedString::from(format!(
            "{}: {}–{}",
            tr(error.field),
            error.minimum,
            error.maximum
        ))
    })?;
    if desktop
        && crate::shortcuts::Shortcuts::current()
            .is_some_and(|keys| !keys.conflicts(&settings.boss_key).is_empty())
    {
        return Err(tr("settings.bossKeyConflictBlock"));
    }
    Ok(settings)
}

fn save(
    session: SessionHandle,
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
) {
    let tasks = super::Tasks::provide();
    if tasks.pending.get_untracked() {
        return;
    }
    if let Some(keys) = crate::shortcuts::Shortcuts::current() {
        keys.cancel_capture();
    }
    let state = session.with(|session| session.state.clone());
    let pending = draft.get_untracked();
    let desktop = is_desktop();
    let proposed = match validate(pending.clone(), state.settings(), desktop) {
        Ok(settings) => settings,
        Err(error) => {
            message.set(error);
            return;
        }
    };
    let old_boss = state.settings().boss_key;
    let boss_changed = desktop && proposed.boss_key != old_boss;
    let page_owner = Owner::current();
    message.set(SharedString::default());
    tasks.pending.set(true);
    busy.set(true);
    tasks.owner.run(move || {
        spawn(async move {
            if boss_changed
                && let Err(error) = crate::boss_key::apply(proposed.boss_key).await
            {
                log::error!("[NSH settings] global shortcut registration failed: {error}");
                if page_owner.is_live() {
                    message.set(error);
                    busy.set(false);
                }
                tasks.pending.set(false);
                return;
            }
            let result = bridge::request(async move {
                hongguo_service::runtime::spawn_blocking(move || {
                    let settings = pending.apply(state.settings(), desktop).map_err(|error| {
                        hongguo_common::AppError::InvalidArgs(format!(
                            "{}: {}–{}", error.field, error.minimum, error.maximum
                        ))
                    })?;
                    hongguo_service::settings_service::save(&state, settings)
                })
                .await
                .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
            })
            .await;
            match result {
                Ok(settings) => {
                    set_color_scheme(match settings.theme.as_str() {
                        "light" => nsh_core::platform::Appearance::Light,
                        "dark" => nsh_core::platform::Appearance::Dark,
                        _ => nsh_core::platform::Appearance::System,
                    });
                    session.notify_identity();
                    if page_owner.is_live() {
                        draft.set(Draft::from(settings));
                        message.set(tr("settings.saved"));
                    }
                }
                Err(error) => {
                    let mut notice = error.text();
                    if boss_changed
                        && let Err(rollback) = crate::boss_key::apply(old_boss).await
                    {
                        log::error!(
                            "[NSH settings] restoring the previous global shortcut failed: {rollback}"
                        );
                        notice = format!(
                            "{notice}\n{}",
                            tr("nsh.bossKeyRollbackFailed").replace("{error}", &rollback)
                        )
                        .into();
                    }
                    log::error!("[NSH settings] save failed: {notice}");
                    if page_owner.is_live() {
                        message.set(notice);
                    }
                }
            }
            if page_owner.is_live() {
                busy.set(false);
            }
            tasks.pending.set(false);
        })
    });
}
