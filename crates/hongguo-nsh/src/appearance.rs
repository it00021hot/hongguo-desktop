use crate::{bridge, events::SessionHandle, i18n::tr};
use nsh_core::{platform::Appearance, prelude::*};

pub(crate) fn save(session: SessionHandle, appearance: Appearance, pending: Signal<bool>) {
    if pending.get_untracked() {
        return;
    }
    pending.set(true);
    let state = session.with(|session| session.state.clone());
    spawn(async move {
        let result = bridge::request(async move {
            hongguo_service::runtime::spawn_blocking(move || {
                let mut settings = state.settings();
                settings.theme = match appearance {
                    Appearance::Light => "light",
                    Appearance::Dark => "dark",
                    Appearance::System => "auto",
                }
                .to_owned();
                hongguo_service::settings_service::save(&state, settings)
            })
            .await
            .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
        })
        .await;
        pending.set(false);
        match result {
            Ok(_) => set_color_scheme(appearance),
            Err(error) => {
                let notice = nsh_base::alert_dialog()
                    .title(tr("settings.save").to_string())
                    .description(tr("settings.saveFailed").replace("{message}", &error.text()))
                    .cancel("close", tr("common.close"))
                    .show();
                if let Err(error) = notice.await {
                    log::error!("[NSH appearance] could not present save failure: {error}");
                }
            }
        }
    });
}
