//! 登录相关 command：发码 / 短信登录 / MFA 轮询 / 状态 / 退出。
//!
//! 只做参数转调；登录编排（发码 / 提交 / MFA 后台轮询 / 账号落库）在 [`hongguo_service::login_service::flow`]——GPUI 壳消费同一份。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;
use hongguo_service::login_service::flow::{self, LoginResult};

/// 发送短信验证码。发码会话的 csrf 凭据由 service 暂存（提交时消费）。
#[tauri::command]
pub async fn login_send_code(
    state: State<'_, AppState>,
    mobile: String,
) -> AppResult<hongguo_api::login::SendCodeOutcome> {
    flow::send_code(state.inner(), &mobile).await
}

/// 短信验证码登录。成功即落库账号；MFA 需二次验证时转后台轮询并返回上下文。会话绑定走发码下发的 csrf Cookie。
#[tauri::command]
pub async fn login_sms_login(
    state: State<'_, AppState>,
    mobile: String,
    code: String,
    mfa_retry_tag: Option<String>,
    mfa_sms_code_key: Option<String>,
) -> AppResult<LoginResult> {
    let mfa = match (&mfa_retry_tag, &mfa_sms_code_key) {
        (Some(t), Some(k)) => Some((t.as_str(), k.as_str())),
        _ => None,
    };
    flow::submit(state.inner(), &mobile, &code, mfa).await
}

/// MFA 上行短信验证轮询（单次调用；前端 3 秒间隔重发）。轮询到 `registered` 后**自动重登**换取会话并落库（用户只需回复短信）。
#[tauri::command]
pub async fn login_mfa_verify(state: State<'_, AppState>) -> AppResult<LoginResult> {
    flow::mfa_verify_once(state.inner()).await
}

/// 取消进行中的 MFA 验证（后台轮询循环会随流程清空自行退出）。
#[tauri::command]
pub fn login_mfa_cancel(state: State<'_, AppState>) -> AppResult<()> {
    flow::cancel_mfa(state.inner());
    Ok(())
}

/// 登录状态（含用户信息；未登录为 None）。
#[tauri::command]
pub fn login_status(
    state: State<'_, AppState>,
) -> AppResult<Option<hongguo_common::model::AccountState>> {
    Ok(flow::status(state.inner()))
}

/// 校验会话有效性并刷新用户信息（会话过期返回错误）。
#[tauri::command]
pub async fn login_user_info(
    state: State<'_, AppState>,
) -> AppResult<hongguo_api::login::PassportUser> {
    flow::user_info(state.inner()).await
}

/// 退出登录（清空账号设置；退出是纯本地操作，无网络请求）。
#[tauri::command]
pub fn login_logout(state: State<'_, AppState>) -> AppResult<()> {
    flow::logout(state.inner())
}
