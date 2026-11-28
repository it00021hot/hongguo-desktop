use crate::{bridge, events::SessionHandle, i18n::tr};
use nsh_core::{platform::Appearance, prelude::*};

pub(crate) fn save(session: SessionHandle, appearance: Appearance, pending: Signal<bool>) {
    if pending.get_untracked() {
        return;
    }
    pending.set(true);
    let feedback = crate::ui::window_notice();
    let state = session.with(|session| session.state.clone());
    spawn(async move {
        let result = bridge::submit(async move {
            hongguo_service::runtime::spawn_blocking(move || {
                hongguo_service::settings_service::update(&state, |mut settings| {
                    settings.theme = match appearance {
                        Appearance::Light => "light",
                        Appearance::Dark => "dark",
                        Appearance::System => "auto",
                    }
                    .to_owned();
                    Ok(settings)
                })
            })
            .await
            .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
        })
        .await;
        pending.set(false);
        match result {
            Ok(_) => set_color_scheme(appearance),
            Err(error) => {
                feedback.update(|message| {
                    *message = tr("settings.saveFailed")
                        .replace("{message}", &error.text())
                        .into()
                });
            }
        }
    });
}
