//! 账号会话落库与资料刷新。

use crate::app_state::AppState;
use hongguo_api::login::{self, PassportUser};
use hongguo_common::model::AccountState;
use hongguo_common::AppResult;

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

/// 账号设置写入（None = 清空登录态）：规范化 → 内存即时替换 → 落库。
pub fn persist_account(state: &AppState, account: Option<AccountState>) -> AppResult<()> {
    let mut settings = state.settings();
    settings.account = account;
    let normalized = crate::settings_service::normalize(settings)?;
    state.replace_settings(normalized.clone());
    state.store.save_settings(&normalized)?;
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
pub fn refresh_profile_after_login(state: AppState) {
    crate::runtime::spawn(async move {
        match login::user_info(&state.api_env()).await {
            Ok(user) => {
                if let Some(mut acc) = state.settings().account {
                    if !user.name.is_empty() {
                        acc.user_name = user.name;
                    }
                    if !user.avatar_url.is_empty() {
                        acc.avatar_url = user.avatar_url;
                    }
                    acc.raw_profile = user.raw;
                    if let Err(e) = persist_account(&state, Some(acc)) {
                        log::warn!("[Login] 登录后资料刷新落库失败: {e}");
                    } else {
                        log::info!("[Login] 登录后资料已刷新（user_info）");
                    }
                }
            }
            Err(e) => log::warn!("[Login] 登录后资料刷新失败（不影响登录态）: {e}"),
        }
    });
}

