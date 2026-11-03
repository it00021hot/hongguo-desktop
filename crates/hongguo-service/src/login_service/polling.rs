//! MFA 后台轮询：回复短信后自动重登并发状态事件。

use super::account::{account_from_login, complete_login};
use super::csrf::with_csrf;
use super::events::LoginMfaStateEvent;
use crate::app_state::AppState;
use hongguo_api::login::{self, LoginOutcome, MfaContext, MfaFlow, UpsmsState};

/// 轮询上限：超过即报超时退出。
const MFA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
/// 轮询间隔。
const MFA_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// MFA 上行短信验证的展示文案（补上「回复 X 到 Y」；通道号已在 tips 里出现时不再补）。
/// `tips` 是服务端下发的展示内容，原样透传，不做 i18n。
pub fn mfa_tips(ctx: &MfaContext) -> String {
    if ctx.channel_mobile.is_empty() || ctx.tips.contains(&ctx.channel_mobile) {
        ctx.tips.clone()
    } else {
        format!(
            "{}（回复 {} 到 {}）",
            ctx.tips, ctx.sms_content, ctx.channel_mobile
        )
    }
}

/// MFA 后台轮询：`MFA_POLL_INTERVAL` 一次直到 registered / 取消 / 超时。
///
/// 成功即自动重登、落库并发 `login-mfa-state` 事件（success/failed）；
/// 每轮先校验流程指纹（用户重新发起登录会替换流程，旧循环自行退出）。
/// 后台任务走共用任务运行时（任意线程可调，见 [`crate::runtime`]）。
pub fn spawn_mfa_polling(state: AppState) {
    crate::runtime::spawn(async move {
        let deadline = std::time::Instant::now() + MFA_TIMEOUT;
        let mut round: u32 = 0;
        loop {
            if std::time::Instant::now() > deadline {
                *state.login_mfa.write() = None;
                log::warn!("[Login] MFA 5 分钟未确认，超时退出");
                emit(
                    &state,
                    LoginMfaStateEvent::Failed {
                        message_kind: "settings.loginMfaTimeout".into(),
                        message: String::new(),
                    },
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
                            let account =
                                account_from_login(flow.mobile.clone(), cookies, &user, token);
                            if let Err(e) = complete_login(&state, account) {
                                emit(
                                    &state,
                                    LoginMfaStateEvent::Failed {
                                        message_kind: e.i18n_key().into(),
                                        message: e.to_string(),
                                    },
                                );
                                return;
                            }
                            log::info!(
                                "[Login] MFA 自动登录成功: {} ({})",
                                user.name,
                                user.user_id
                            );
                            emit(&state, LoginMfaStateEvent::Success { name: user.name });
                            return;
                        }
                        Ok(LoginOutcome::Mfa(next)) => {
                            *state.login_mfa.write() = Some(MfaFlow {
                                ctx: (*next).clone(),
                                mobile: flow.mobile.clone(),
                                code: flow.code.clone(),
                            });
                            emit(
                                &state,
                                LoginMfaStateEvent::Waiting {
                                    server_tip: mfa_tips(&next),
                                },
                            );
                        }
                        Err(e) => {
                            *state.login_mfa.write() = None;
                            emit(
                                &state,
                                LoginMfaStateEvent::Failed {
                                    message_kind: e.i18n_key().into(),
                                    message: e.to_string(),
                                },
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
            tokio::time::sleep(MFA_POLL_INTERVAL).await;
        }
    });
}

/// 发一个 MFA 状态事件（无订阅方时静默丢弃，无害）。
fn emit(state: &AppState, event: LoginMfaStateEvent) {
    state
        .events()
        .publish(crate::events::ServiceEvent::LoginMfa(event));
}
