//! 登录相关 command：发码 / 短信登录 / MFA 轮询 / 状态 / 退出。

use serde::Serialize;
use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::login::{self, LoginOutcome, PassportUser, UpsmsState};
use crate::domain::model::AccountState;
use crate::error::{AppError, AppResult};

/// 发送短信验证码。
#[tauri::command]
pub async fn login_send_code(state: State<'_, AppState>, mobile: String) -> AppResult<String> {
    validate_mobile(&mobile)?;
    let env = state.api_env();
    login::send_sms_code(&env, &mobile).await
}

/// 短信验证码登录。成功即落库账号；MFA 需二次验证时返回上下文。
#[tauri::command]
pub async fn login_sms_login(
    state: State<'_, AppState>,
    mobile: String,
    code: String,
    // MFA 重试上下文（上一次返回的 retryTag / smsCodeKey）
    mfa_retry_tag: Option<String>,
    mfa_sms_code_key: Option<String>,
) -> AppResult<LoginResult> {
    validate_mobile(&mobile)?;
    if code.trim().is_empty() {
        return Err(AppError::Media("验证码不能为空".into()));
    }
    let env = state.api_env();
    let mfa = match (&mfa_retry_tag, &mfa_sms_code_key) {
        (Some(t), Some(k)) => Some((t.as_str(), k.as_str())),
        _ => None,
    };
    match login::sms_login(&env, &mobile, code.trim(), mfa).await? {
        LoginOutcome::Success { cookies, user } => {
            let account = AccountState {
                mobile: mobile.clone(),
                cookies,
                user_name: user.name.clone(),
                user_id: user.user_id.clone(),
                login_at: chrono::Utc::now().timestamp(),
            };
            persist_account(&state, Some(account))?;
            Ok(LoginResult::Success { user })
        }
        LoginOutcome::Mfa {
            retry_tag,
            sms_code_key,
            tips,
        } => Ok(LoginResult::Mfa {
            retry_tag,
            sms_code_key,
            tips,
        }),
    }
}

/// MFA 上行短信验证轮询（单次调用；前端定时重发）。
#[tauri::command]
pub async fn login_mfa_verify(
    state: State<'_, AppState>,
    retry_tag: String,
    sms_code_key: String,
    mobile: String,
) -> AppResult<LoginResult> {
    let env = state.api_env();
    match login::upsms_verify(&env, &retry_tag, &sms_code_key).await? {
        UpsmsState::Waiting => Ok(LoginResult::MfaWaiting),
        UpsmsState::Success { cookies, user } => {
            let account = AccountState {
                mobile,
                cookies,
                user_name: user.name.clone(),
                user_id: user.user_id.clone(),
                login_at: chrono::Utc::now().timestamp(),
            };
            persist_account(&state, Some(account))?;
            Ok(LoginResult::Success { user })
        }
    }
}

/// 登录状态（含用户信息；未登录为 None）。
#[tauri::command]
pub fn login_status(state: State<'_, AppState>) -> Option<AccountState> {
    state.settings().account
}

/// 校验会话有效性并刷新用户信息（会话过期返回错误）。
#[tauri::command]
pub async fn login_user_info(state: State<'_, AppState>) -> AppResult<PassportUser> {
    let settings = state.settings();
    if settings.account.is_none() {
        return Err(AppError::Media("未登录".into()));
    }
    let env = state.api_env();
    let user = login::user_info(&env).await?;
    if let Some(mut acc) = state.settings().account {
        acc.user_name = user.name.clone();
        acc.user_id = user.user_id.clone();
        persist_account(&state, Some(acc))?;
    }
    Ok(user)
}

/// 退出登录（清空账号设置）。
#[tauri::command]
pub fn login_logout(state: State<'_, AppState>) -> AppResult<()> {
    persist_account(&state, None)
}

/// 登录命令的统一返回：成功 / 需 MFA / MFA 等待中。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoginResult {
    Success { user: PassportUser },
    Mfa { retry_tag: String, sms_code_key: String, tips: String },
    MfaWaiting,
}

/// 手机号形态校验：11 位、1 开头、全数字（不做号段穷举，服务端会再校验）。
fn validate_mobile(mobile: &str) -> AppResult<()> {
    let ok = mobile.len() == 11
        && mobile.starts_with('1')
        && mobile.bytes().all(|b| b.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(AppError::Media("手机号格式不正确".into()))
    }
}

/// 账号设置写入（None = 清空登录态）。
fn persist_account(state: &State<'_, AppState>, account: Option<AccountState>) -> AppResult<()> {
    let mut settings = state.settings();
    settings.account = account;
    let normalized = crate::service::settings_service::normalize(settings)?;
    state.replace_settings(normalized.clone());
    state.store.save_settings(&normalized)?;
    Ok(())
}
