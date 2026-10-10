//! 登录相关 command：发码 / 短信登录 / MFA 轮询 / 状态 / 退出。

use serde::Serialize;
use tauri::{Emitter, State};

use crate::app_state::AppState;
use crate::domain::api::login::{
    self, LoginOutcome, MfaContext, MfaFlow, PassportUser, UpsmsState,
};
use crate::domain::model::AccountState;
use crate::error::{AppError, AppResult};

/// MFA 后台轮询的状态事件（前端 LoginDialog 订阅）。
pub const LOGIN_MFA_STATE: &str = "login-mfa-state";

/// 发送短信验证码。发码会话的 csrf 凭据暂存 AppState（sms_login 消费）。
#[tauri::command]
pub async fn login_send_code(
    state: State<'_, AppState>,
    mobile: String,
) -> AppResult<login::SendCodeOutcome> {
    validate_mobile(&mobile)?;
    let env = state.api_env();
    match login::send_sms_code(&env, &mobile).await {
        Ok(outcome) => {
            log::info!("[Login] 发码成功: {}", outcome.message);
            if !outcome.csrf_cookie.is_empty() {
                *state.login_csrf.write() = Some(outcome.csrf_cookie.clone());
            }
            Ok(outcome)
        }
        Err(e) => {
            log::warn!("[Login] 发码失败: {e}");
            Err(e)
        }
    }
}

/// 短信验证码登录。成功即落库账号；MFA 需二次验证时返回上下文。
///
/// `mobile_ticket` 参数仅为前端兼容保留（1.1.3 实测会话绑定走 csrf
/// Cookie，ticket 不再回传服务端）。
#[tauri::command]
pub async fn login_sms_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mobile: String,
    code: String,
    // MFA 重试上下文（上一次返回的 retryTag / smsCodeKey）
    mfa_retry_tag: Option<String>,
    mfa_sms_code_key: Option<String>,
    mobile_ticket: Option<String>,
) -> AppResult<LoginResult> {
    let _ = mobile_ticket; // 已废弃：保留参数位避免前端 breaking
    validate_mobile(&mobile)?;
    if code.trim().is_empty() {
        return Err(AppError::Auth("验证码不能为空".into()));
    }
    let env = with_csrf(&state, state.api_env());
    let mfa = match (&mfa_retry_tag, &mfa_sms_code_key) {
        (Some(t), Some(k)) => Some((t.as_str(), k.as_str())),
        _ => None,
    };
    let outcome = login::sms_login(&env, &mobile, code.trim(), mfa).await;
    if let Err(e) = &outcome {
        log::warn!("[Login] 登录失败: {e}");
    }
    match outcome? {
        LoginOutcome::Success {
            cookies,
            user,
            token,
        } => {
            log::info!("[Login] 登录成功: {} ({})", user.name, user.user_id);
            // 登录成功即消费掉 csrf / MFA 流程（一次性凭据）
            *state.login_csrf.write() = None;
            *state.login_mfa.write() = None;
            let account = AccountState {
                mobile: mobile.clone(),
                cookies,
                user_name: user.name.clone(),
                avatar_url: user.avatar_url.clone(),
                user_id: user.user_id.clone(),
                login_at: chrono::Utc::now().timestamp(),
                token: token.clone(),
                raw_login: user.raw.clone(),
                raw_profile: String::new(),
            };
            persist_account(&state, Some(account))?;
            Ok(LoginResult::Success { user })
        }
        LoginOutcome::Mfa(ctx) => {
            // 暂存流程并转后台轮询：用户回复短信后自动重登并发事件——
            // 前端界面关掉也不丢状态（2026-10-04 体验事故修复）
            *state.login_mfa.write() = Some(MfaFlow {
                ctx: (*ctx).clone(),
                mobile,
                code: code.trim().to_string(),
            });
            spawn_mfa_polling(app, state.inner().clone());
            Ok(mfa_result(&ctx))
        }
    }
}

/// MFA 后台轮询：3s 一次直到 registered / 取消 / 超时（5 分钟）。
///
/// 成功即自动重登、落库并发 `login-mfa-state` 事件（success/failed）；
/// 每轮先校验流程指纹（用户重新发起登录会替换流程，旧循环自行退出）。
fn spawn_mfa_polling(app: tauri::AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        let mut round: u32 = 0;
        loop {
            if std::time::Instant::now() > deadline {
                *state.login_mfa.write() = None;
                log::warn!("[Login] MFA 5 分钟未确认，超时退出");
                let _ = app.emit(
                    LOGIN_MFA_STATE,
                    serde_json::json!({"state": "failed", "message": "验证超时，请重新登录"}),
                );
                return;
            }
            let Some(flow) = state.login_mfa.read().clone() else {
                return; // 用户取消 / 退出登录
            };
            let fingerprint = (flow.ctx.retry_tag.clone(), flow.ctx.log_id.clone());
            let env = with_csrf(&state, state.api_env());
            round += 1;
            match login::upsms_verify(&flow.ctx, &env).await {
                Ok(UpsmsState::Registered { mfa_token, .. }) => {
                    log::info!("[Login] MFA 第 {round} 轮 registered，自动重登");
                    // registered 响应下发新 mfa_token，重登必须带它
                    let mut ctx = flow.ctx.clone();
                    ctx.mfa_token = mfa_token;
                    match login::mfa_relogin(&env, &flow.mobile, &flow.code, &ctx).await {
                        Ok(LoginOutcome::Success {
                            cookies,
                            user,
                            token,
                        }) => {
                            *state.login_mfa.write() = None;
                            *state.login_csrf.write() = None;
                            let account = AccountState {
                                mobile: flow.mobile.clone(),
                                cookies,
                                user_name: user.name.clone(),
                                avatar_url: user.avatar_url.clone(),
                                user_id: user.user_id.clone(),
                                login_at: chrono::Utc::now().timestamp(),
                                token,
                                raw_login: user.raw.clone(),
                                raw_profile: String::new(),
                            };
                            if let Err(e) = persist_account(&state, Some(account)) {
                                let _ = app.emit(
                                    LOGIN_MFA_STATE,
                                    serde_json::json!({"state": "failed", "message": e.to_string()}),
                                );
                                return;
                            }
                            log::info!(
                                "[Login] MFA 自动登录成功: {} ({})",
                                user.name,
                                user.user_id
                            );
                            let _ = app.emit(
                                LOGIN_MFA_STATE,
                                serde_json::json!({"state": "success", "name": user.name}),
                            );
                            return;
                        }
                        Ok(LoginOutcome::Mfa(next)) => {
                            // 新一轮验证（换了方式）：替换流程继续等
                            *state.login_mfa.write() = Some(MfaFlow {
                                ctx: (*next).clone(),
                                mobile: flow.mobile.clone(),
                                code: flow.code.clone(),
                            });
                            let tips = match mfa_result(&next) {
                                LoginResult::Mfa { tips, .. } => tips,
                                _ => String::new(),
                            };
                            let _ = app.emit(
                                LOGIN_MFA_STATE,
                                serde_json::json!({"state": "waiting", "message": tips}),
                            );
                        }
                        Err(e) => {
                            *state.login_mfa.write() = None;
                            let _ = app.emit(
                                LOGIN_MFA_STATE,
                                serde_json::json!({"state": "failed", "message": e.to_string()}),
                            );
                            return;
                        }
                    }
                }
                Ok(UpsmsState::Waiting) => {
                    // 30 秒打一条，用户在等的时候日志可查轮询是否活着
                    if round % 10 == 1 {
                        log::info!("[Login] MFA 第 {round} 轮：等待用户回复短信");
                    }
                }
                Err(e) => {
                    // 单次网络抖动不致命：流程还在就继续等，直到超时
                    log::warn!("[Login] MFA 轮询出错（继续等待）: {e}");
                }
            }
            // 指纹变了说明用户发起了新登录，旧循环退出
            let current = state.login_mfa.read().clone();
            if let Some(cur) = current
                && (cur.ctx.retry_tag, cur.ctx.log_id) != fingerprint
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });
}

/// 把暂存的 csrf 并进调用环境（覆盖同名 cookie 字段，缺失则追加）。
fn with_csrf(
    state: &AppState,
    mut env: crate::domain::api::client::ApiEnv,
) -> crate::domain::api::client::ApiEnv {
    let Some(csrf) = state.login_csrf.read().clone() else {
        return env;
    };
    let base = env.cookie.unwrap_or_default();
    let mut fields: Vec<(String, String)> = base
        .split("; ")
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    for key in ["passport_csrf_token", "passport_csrf_token_default"] {
        match fields.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = csrf.clone(),
            None => fields.push((key.to_string(), csrf.clone())),
        }
    }
    env.cookie = Some(
        fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; "),
    );
    env
}

/// MFA 上行短信验证轮询（单次调用；前端 3 秒间隔重发）。
///
/// 轮询到 `registered` 后**自动重登**换取会话并落库（用户只需回复短信）。
#[tauri::command]
pub async fn login_mfa_verify(state: State<'_, AppState>) -> AppResult<LoginResult> {
    let flow = state
        .login_mfa
        .read()
        .clone()
        .ok_or_else(|| AppError::Auth("没有进行中的 MFA 验证，请重新登录".into()))?;
    let env = with_csrf(&state, state.api_env());
    match login::upsms_verify(&flow.ctx, &env).await? {
        UpsmsState::Waiting => Ok(LoginResult::MfaWaiting),
        UpsmsState::Registered { .. } => {
            match login::mfa_relogin(&env, &flow.mobile, &flow.code, &flow.ctx).await? {
                LoginOutcome::Success {
                    cookies,
                    user,
                    token,
                } => {
                    *state.login_mfa.write() = None;
                    *state.login_csrf.write() = None;
                    let account = AccountState {
                        mobile: flow.mobile.clone(),
                        cookies,
                        user_name: user.name.clone(),
                        avatar_url: user.avatar_url.clone(),
                        user_id: user.user_id.clone(),
                        login_at: chrono::Utc::now().timestamp(),
                        token,
                        raw_login: user.raw.clone(),
                        raw_profile: String::new(),
                    };
                    persist_account(&state, Some(account))?;
                    Ok(LoginResult::Success { user })
                }
                LoginOutcome::Mfa(ctx) => {
                    // 新一轮 MFA（换了一种验证方式）：更新流程继续轮询
                    *state.login_mfa.write() = Some(MfaFlow {
                        ctx: (*ctx).clone(),
                        mobile: flow.mobile.clone(),
                        code: flow.code.clone(),
                    });
                    Ok(mfa_result(&ctx))
                }
            }
        }
    }
}

/// Mfa 上下文 → 前端返回（展示回复方式，凭据不回传）。
fn mfa_result(ctx: &MfaContext) -> LoginResult {
    LoginResult::Mfa {
        retry_tag: ctx.retry_tag.clone(),
        sms_code_key: ctx.sms_code_key.clone(),
        channel_mobile: ctx.channel_mobile.clone(),
        sms_content: ctx.sms_content.clone(),
        tips: if ctx.channel_mobile.is_empty() || ctx.tips.contains(&ctx.channel_mobile) {
            ctx.tips.clone()
        } else {
            format!(
                "{}（回复 {} 到 {}）",
                ctx.tips, ctx.sms_content, ctx.channel_mobile
            )
        },
    }
}

/// 取消进行中的 MFA 验证（后台轮询循环会随流程清空自行退出）。
#[tauri::command]
pub fn login_mfa_cancel(state: State<'_, AppState>) -> AppResult<()> {
    *state.login_mfa.write() = None;
    Ok(())
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
        return Err(AppError::Auth("未登录".into()));
    }
    let env = state.api_env();
    let user = login::user_info(&env).await?;
    if let Some(mut acc) = state.settings().account {
        acc.user_name = user.name.clone();
        // ⚠️ user_id 绝不能直接用响应里的 user_id 刷新：user_info 端点返回的
        // 是 encode_user_id 加密形态（#c1967_… 71 字符），数字 uid 只在
        // sms_login 响应里——覆盖会让「删除自己的评论」的 isMine 判定
        // （对比评论作者数字 user_id）永远不成立（2026-10-10 实测事故）。
        // 但存量坏值可以从本响应的 req_id（字段名伪装的数字 uid，
        // 2026-10-10 实测与 sms_login 的数字 user_id 同值）就地自愈，
        // 不用退出重登。
        if (acc.user_id.contains('#') || acc.user_id.is_empty())
            && let Some(numeric) = login::numeric_uid_from_raw(&user.raw)
        {
            log::info!("[Login] user_id 数字形态自愈（req_id）: {numeric}");
            acc.user_id = numeric;
        }
        // user_info 偶发不带头像时不清空已有值
        if !user.avatar_url.is_empty() {
            acc.avatar_url = user.avatar_url.clone();
        }
        // 原文整体落库：红果号（biz_user_id）/vip_info 等 60+ 长尾字段
        // 按需从 raw_profile 读取，不再为个别字段加解析路径
        acc.raw_profile = user.raw.clone();
        persist_account(&state, Some(acc))?;
    }
    Ok(user)
}

/// 退出登录（清空账号设置；退出是纯本地操作，1.1.3 实测无网络请求）。
#[tauri::command]
pub fn login_logout(state: State<'_, AppState>) -> AppResult<()> {
    *state.login_csrf.write() = None;
    *state.login_mfa.write() = None;
    persist_account(&state, None)
}

/// 登录命令的统一返回：成功 / 需 MFA / MFA 等待中。
#[derive(Debug, Clone, Serialize)]
// enum 级 rename_all 只作用于变体名（tag 值）；变体字段必须各自
// 再标一次 camelCase——否则 Mfa 序列化成 retry_tag 之类 snake_case，
// 前端 zod 校验直接挂（2026-10-04「数据格式异常」事故）
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoginResult {
    #[serde(rename_all = "camelCase")]
    Success {
        user: PassportUser,
    },
    #[serde(rename_all = "camelCase")]
    Mfa {
        retry_tag: String,
        sms_code_key: String,
        /// 上行短信通道号（如 9515211003）
        channel_mobile: String,
        /// 要回复的内容（如 "YZ"）
        sms_content: String,
        tips: String,
    },
    MfaWaiting,
}

/// 手机号形态校验：11 位、1 开头、全数字（不做号段穷举，服务端会再校验）。
fn validate_mobile(mobile: &str) -> AppResult<()> {
    let ok =
        mobile.len() == 11 && mobile.starts_with('1') && mobile.bytes().all(|b| b.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(AppError::Auth("手机号格式不正确".into()))
    }
}

/// 账号设置写入（None = 清空登录态）。
fn persist_account(state: &AppState, account: Option<AccountState>) -> AppResult<()> {
    let mut settings = state.settings();
    settings.account = account;
    let normalized = crate::service::settings_service::normalize(settings)?;
    state.replace_settings(normalized.clone());
    state.store.save_settings(&normalized)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mfa 变体的序列化形状必须与前端 loginResultSchema 逐字段一致
    /// （一次 zod 校验失败事故的回归锁）。channel_mobile 用替换后的
    /// 真实通道号（parse 层已做 95→106 修正，这里锁序列化形状）。
    #[test]
    fn mfa_serializes_to_frontend_shape() {
        let r = mfa_result(&crate::domain::api::login::MfaContext {
            retry_tag: "1".into(),
            sms_code_key: "k".into(),
            channel_mobile: "10691859839103".into(),
            sms_content: "YZ".into(),
            tips: "回复 YZ 到 10691859839103".into(),
            ..Default::default()
        });
        let j = serde_json::to_string(&r).unwrap();
        println!("Mfa JSON = {j}");
        assert!(j.contains(r#""kind":"mfa""#), "tag 必须是 mfa: {j}");
        assert!(j.contains(r#""retryTag":"1""#), "字段必须 camelCase: {j}");
        assert!(j.contains(r#""smsCodeKey":"k""#), "{j}");
        assert!(j.contains(r#""channelMobile":"10691859839103""#), "{j}");
        assert!(j.contains(r#""smsContent":"YZ""#), "{j}");
        assert!(j.contains("10691859839103"), "{j}");
    }
}
