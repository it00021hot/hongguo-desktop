//! 账号会话落库与资料刷新。

use crate::app_state::AppState;
use crate::events::ServiceEvent;
use hongguo_api::login::{self, PassportUser};
use hongguo_common::AppResult;
use hongguo_common::model::AccountState;

/// 登录成功产物 → 账号快照（`login_at` 用 UTC 毫秒时间戳，与全库其它时间字段一致）。
/// user_id **绝不能**用 user_info 响应里的 user_id 刷新：那是 encode_user_id 加密形态，
/// 数字 uid 只在 sms_login 响应里——覆盖会让评论 isMine 判定永远不成立。
/// 存量坏值在登录时从响应的 req_id（伪装字段名的数字 uid）就地自愈。
pub fn account_from_login(
    mobile: String,
    cookies: String,
    user: &PassportUser,
    token: String,
) -> AccountState {
    AccountState {
        mobile,
        cookies,
        user_name: user.name.clone(),
        avatar_url: user.avatar_url.clone(),
        user_id: user.user_id.clone(),
        login_at: chrono::Utc::now().timestamp_millis(),
        token,
        raw_login: user.raw.clone(),
        raw_profile: String::new(),
    }
}

/// 账号设置写入（None = 清空登录态）：规范化 → 落库 → 更新内存。
pub fn persist_account(state: &AppState, account: Option<AccountState>) -> AppResult<()> {
    let mut current = state.settings.write();
    let mut settings = current.clone();
    settings.account = account;
    let normalized = crate::settings_service::normalize(settings)?;
    state.store.save_settings(&normalized)?;
    *current = normalized;
    Ok(())
}

/// 登录成功收尾：消费一次性凭据（csrf / MFA 流程）、落库账号、后台刷新资料与离线历史补传。
pub fn complete_login(state: &AppState, account: AccountState) -> AppResult<()> {
    *state.login_csrf.write() = None;
    *state.login_mfa.write() = None;
    persist_account(state, Some(account))?;
    refresh_profile_after_login(state.clone());
    let st = state.clone();
    crate::runtime::spawn(async move { crate::history_service::upload_pending(&st).await });
    Ok(())
}

/// 登录成功后跟进刷新用户资料（best-effort）：sms_login 响应给的是默认昵称 + 低清头像，
/// 真昵称/高清头像/raw_profile 都在 user_info；失败只记日志，不让登录报错。
fn refresh_profile_after_login(state: AppState) {
    let settings = state.settings();
    let Some(expected) = settings.account.clone() else {
        return;
    };
    let env = state.api_env_from_settings(settings);
    crate::runtime::spawn(async move {
        match login::user_info(&env).await {
            Ok(user) => match apply_profile(&state, &expected, user) {
                Ok(true) => state.events().publish(ServiceEvent::AccountProfileChanged),
                Ok(false) => {}
                Err(e) => log::warn!("[Login] 登录后资料刷新落库失败: {e}"),
            },
            Err(e) => log::warn!("[Login] 登录后资料刷新失败（不影响登录态）: {e}"),
        }
    });
}

fn apply_profile(state: &AppState, expected: &AccountState, user: PassportUser) -> AppResult<bool> {
    let mut current = state.settings.write();
    let mut settings = current.clone();
    let Some(account) = settings.account.as_mut().filter(|account| {
        account.login_at == expected.login_at
            && account.user_id == expected.user_id
            && account.cookies == expected.cookies
    }) else {
        return Ok(false);
    };
    // 请勿从 user_info 复制 user_id：响应返回的不是登录时的数字 uid。
    if !user.name.is_empty() {
        account.user_name = user.name;
    }
    if !user.avatar_url.is_empty() {
        account.avatar_url = user.avatar_url;
    }
    account.raw_profile = user.raw;
    if settings.account == current.account {
        return Ok(false);
    }
    state.store.save_settings(&settings)?;
    *current = settings;
    Ok(true)
}
