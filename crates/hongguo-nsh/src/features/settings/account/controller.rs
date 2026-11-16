use super::{Action, State};
use crate::{bridge, events::SessionHandle, i18n::tr, navigation::AccountPrompt};
use hongguo_service::login_service::flow;
use nsh_core::prelude::*;

pub(super) fn dispatch(
    session: SessionHandle,
    state: State,
    prompt: AccountPrompt,
) -> impl Fn(Action) + Copy {
    move |action| {
        if state.busy.get_untracked() {
            return;
        }
        if matches!(action, Action::Send)
            && (state.countdown() > 0 || !super::model::valid_mobile(&state.mobile.get_untracked()))
        {
            return;
        }
        if matches!(action, Action::Login) && !state.can_login() {
            return;
        }
        state.busy.set(true);
        state.feedback.set(SharedString::default());
        session.clear_login_notice();
        let mobile = state.mobile.get_untracked();
        let code = state.code.get_untracked();
        let app = session.with(|session| session.state.clone());
        spawn(async move {
            let result = bridge::request(async move {
                match action {
                    Action::Send => flow::send_code(&app, &mobile)
                        .await
                        .map(|outcome| Some((mobile, outcome.retry_time))),
                    Action::Login => flow::submit(&app, &mobile, &code, None).await.map(|_| None),
                    Action::CancelMfa => {
                        flow::cancel_mfa(&app);
                        Ok(None)
                    }
                }
            })
            .await;
            match result {
                Ok(Some((mobile, retry))) => {
                    let now = hongguo_common::utils::time::now_ms();
                    state.now.set(now);
                    state
                        .deadlines
                        .update(|deadlines| deadlines.record(mobile, retry, now));
                    state.code_focus.update(|version| *version += 1);
                    state.feedback.set(
                        tr("settings.codeSent")
                            .replace("{s}", &state.countdown().to_string())
                            .into(),
                    );
                }
                Ok(None) => {
                    state.code.set(String::new());
                    session.notify_identity();
                    if session.with(|session| session.state.settings().account.is_some()) {
                        prompt.set_open(false);
                        session.notify_library();
                    }
                }
                Err(error) => state.feedback.set(error.text()),
            }
            state.busy.set(false);
        });
    }
}
