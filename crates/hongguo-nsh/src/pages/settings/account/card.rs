use crate::{events::SessionHandle, i18n::tr, navigation::AccountPrompt, ui};
use hongguo_common::model::AccountState;
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
pub(super) struct Profile {
    pub(super) name: String,
    avatar: String,
    mobile: String,
    hongguo_id: String,
}

impl From<AccountState> for Profile {
    fn from(account: AccountState) -> Self {
        let hongguo_id = serde_json::from_str::<serde_json::Value>(&account.raw_profile)
            .ok()
            .and_then(|value| value.get("data")?.get("biz_user_id").cloned())
            .and_then(|id| match id {
                serde_json::Value::String(id) => Some(id),
                serde_json::Value::Number(id) => Some(id.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        Self {
            name: if account.user_name.is_empty() {
                account.user_id
            } else {
                account.user_name
            },
            avatar: account.avatar_url,
            mobile: if super::model::valid_mobile(&account.mobile) {
                format!("{}****{}", &account.mobile[..3], &account.mobile[7..])
            } else {
                account.mobile
            },
            hongguo_id,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Context {
    pub(super) account: Memo<Option<Profile>>,
    pub(super) logout: Signal<bool>,
    pub(super) pending: Signal<bool>,
}

pub(super) fn provide(session: SessionHandle) -> View {
    let state = Context {
        account: computed(move || {
            session.identity();
            session.with(|session| session.state.settings().account.map(Profile::from))
        }),
        logout: signal(false),
        pending: signal(false),
    };
    provide_context(state);
    confirmation(state, move || {
        if state.pending.get_untracked() {
            return;
        }
        state.pending.set(true);
        let app = session.with(|session| session.state.clone());
        spawn(async move {
            let result = crate::bridge::request(async move {
                hongguo_service::runtime::spawn_blocking(move || {
                    hongguo_service::login_service::flow::logout(&app)
                })
                .await
                .map_err(|error| hongguo_common::AppError::Internal(error.to_string()))?
            })
            .await;
            match result {
                Ok(()) => {
                    session.notify_identity();
                    session.notify_library();
                }
                Err(error) => {
                    let notice = nsh_base::alert_dialog()
                        .title(tr("settings.logout"))
                        .description(error.text())
                        .action("close", tr("common.close"))
                        .show();
                    spawn(async move {
                        if let Err(error) = notice.await {
                            log::error!("[NSH account] logout notice: {error}");
                        }
                    });
                }
            }
            state.pending.set(false);
        });
    })
}

pub(super) fn provide_preview() -> View {
    let state = Context {
        account: computed(|| None),
        logout: signal(false),
        pending: signal(false),
    };
    provide_context(state);
    confirmation(state, || {})
}

fn confirmation(state: Context, logout: impl Fn() + 'static) -> View {
    view().child(ui::confirmation(
        state.logout,
        move || tr("nav.logoutConfirmTitle"),
        move || tr("settings.logout"),
        move || {
            text(move || tr("nav.logoutConfirmBody"))
                .text_sm()
                .line_height(px(20.0))
                .text_color(move || theme().muted_foreground)
        },
        logout,
    ))
}

pub(super) fn build() -> View {
    let state = use_context::<Context>().expect("account dialog owner provides account card state");
    let prompt = AccountPrompt::provide();
    ui::form_surface(ui::account_title()).child(dynamic(move || match state.account.get() {
        None => view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(ui::form_hint(move || tr("settings.loginPrompt")).flex_1())
            .child(
                ui::form_icon_action(move || tr("settings.loginAction"), ui::FormAction::Login)
                    .on_press(move || prompt.request()),
            )
            .into_element(),
        Some(account) => view()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_3()
            .children((!account.avatar.is_empty()).then(|| {
                image(nsh_core::element::ImageSrc::from(account.avatar))
                    .size_8()
                    .rounded_full()
                    .object_fit(nsh_core::ResizeMode::Cover)
            }))
            .child(ui::account_badge(account.name))
            .children((!account.hongguo_id.is_empty()).then(|| {
                view()
                    .flex_row()
                    .gap_1()
                    .child(
                        text(move || format!("{}:", tr("settings.hongguoId")))
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(
                        text(account.hongguo_id)
                            .font_family("monospace")
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                    )
            }))
            .child(
                text(account.mobile)
                    .text_xs()
                    .font_family("monospace")
                    .text_color(move || theme().muted_foreground),
            )
            .child(view().flex_1())
            .child(
                ui::account_logout(move || tr("settings.logout"))
                    .disabled(state.pending)
                    .on_press(move || state.logout.set(true)),
            )
            .into_element(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;
    fn account(raw_profile: &str) -> AccountState {
        AccountState {
            user_id: "uid".into(),
            mobile: "13800138000".into(),
            raw_profile: raw_profile.into(),
            cookies: String::new(),
            user_name: String::new(),
            avatar_url: String::new(),
            login_at: 0,
            token: String::new(),
            raw_login: String::new(),
        }
    }
    #[test]
    fn account_card_keeps_large_hongguo_ids_exact_and_masks_mobile_without_credentials() {
        let profile = Profile::from(account(r#"{"data":{"biz_user_id":9007199254740993}}"#));
        assert_eq!(profile.name, "uid");
        assert_eq!(profile.hongguo_id, "9007199254740993");
        assert_eq!(profile.mobile, "138****8000");
        let profile = Profile::from(account("not JSON"));
        assert_eq!(profile.hongguo_id, "");
    }

    #[component]
    fn SignedInCard() -> impl IntoElement {
        let account = signal(Some(Profile::from(account(
            r#"{"data":{"biz_user_id":"9007199254740993"}}"#,
        ))));
        let state = Context {
            account: computed(move || account.get()),
            logout: signal(false),
            pending: signal(false),
        };
        provide_context(state);
        view()
            .flex_1()
            .child(build())
            .child(confirmation(state, move || account.set(None)))
    }

    #[test]
    fn account_logout_confirms_scope_and_cancel_keeps_the_authenticated_card() -> Result<(), String>
    {
        let mut config = EngineConfig::new(Root::of::<SignedInCard>().tokens(nsh_base::tokens_of(
            Some(include_str!("../../../../theme.json")),
        )?));
        config.environment.os = nsh_core::platform::Os::MacOs;
        config.environment.reduce_motion = true;
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.set_viewport(size(px(1000.0), px(840.0)))?;
        app.wait_for("138****8000")?;
        app.wait_for("9007199254740993")?;
        app.press("role:button[Sign out]")?;
        app.wait_for("id:confirmation")?;
        app.press("role:button[Cancel]")?;
        app.wait_for("138****8000")?;
        app.press("role:button[Sign out]")?;
        app.press("id:confirmation >> role:button[Sign out]")?;
        app.wait_for("role:button[Sign in]")?;
        assert!(!app.shows("138****8000"));
        Ok(())
    }
}
