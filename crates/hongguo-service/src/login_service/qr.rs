use crate::app_state::AppState;
use hongguo_api::login::{self, qr};
use hongguo_common::{AppError, AppResult, model::AccountState};

pub struct Attempt(qr::Attempt);

pub struct Confirmed(AccountState);

pub enum Status {
    Waiting,
    Scanned,
    Cancelled,
    Expired,
    Rotated(String),
    Confirmed(Confirmed),
}

pub async fn start(state: &AppState) -> AppResult<(Attempt, String)> {
    let settings = state.settings();
    if settings.account.is_some() || state.login_mfa.read().is_some() {
        return Err(AppError::Auth(
            "An account or verification is active".into(),
        ));
    }
    let (attempt, image) = qr::Attempt::start(&settings.proxy).await?;
    Ok((Attempt(attempt), image))
}

impl Attempt {
    pub async fn poll(&mut self, state: &AppState) -> AppResult<Status> {
        Ok(match self.0.poll().await? {
            qr::Status::Waiting => Status::Waiting,
            qr::Status::Scanned => Status::Scanned,
            qr::Status::Cancelled => Status::Cancelled,
            qr::Status::Expired => Status::Expired,
            qr::Status::Rotated(image) => Status::Rotated(image),
            qr::Status::Confirmed { user, cookies } => {
                let account = super::account::account_from_login(
                    String::new(),
                    cookies,
                    &user,
                    String::new(),
                );
                let mut settings = state.settings();
                if settings.account.is_some() {
                    return Err(AppError::Auth("An account is already active".into()));
                }
                settings.account = Some(account.clone());
                let profile = login::user_info(&state.api_env_from_settings(settings))
                    .await
                    .map_err(|_| AppError::Auth("Mobile account verification failed".into()))?;
                if profile.user_id.is_empty() || profile.user_id == "0" {
                    return Err(AppError::Auth(
                        "The scan did not authorize the mobile account".into(),
                    ));
                }
                Status::Confirmed(Confirmed(account))
            }
        })
    }
}

pub fn finish(state: &AppState, confirmed: Confirmed) -> AppResult<()> {
    if state.settings().account.is_some() || state.login_mfa.read().is_some() {
        return Err(AppError::Auth(
            "An account or verification is active".into(),
        ));
    }
    super::account::complete_login(state, confirmed.0)
}
