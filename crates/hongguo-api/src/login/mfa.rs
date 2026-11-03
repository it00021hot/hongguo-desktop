//! MFA 上行短信验证：upsms/verify 轮询 + registered 后的重登流程。
//! MFA 检测入口在 mod.rs 的 parse_login_response，这里只承载 MFA 场景自己的请求与 cookie 处理。

use serde_json::Value;

use super::model::{MfaContext, UpsmsState};
use super::{
    FORM_CONTENT_TYPE, LoginOutcome, PASSPORT_ORIGIN, error_code_of, extract_cookie_pairs,
    form_urlencoded, passport_sdk_query, sms_login,
};
use crate::client::{ApiEnv, api_call_full_response};
use hongguo_common::{AppError, AppResult};

/// MFA 上行短信的真实可回复通道号：接口下发的 95 扩展号段服务端收不到回复，
/// 按 106 通道回复才能通过 MFA。
const REAL_UPSMS_CHANNEL: &str = "10691859839103";
const STUB_UPSMS_CHANNEL: &str = "9515211003";

/// 上行短信通道号换成真实网关号（未知号码原样透传，服务端未来换号时
/// 再对齐）。
pub(super) fn real_upsms_channel(api_channel: &str) -> &str {
    if api_channel == STUB_UPSMS_CHANNEL {
        REAL_UPSMS_CHANNEL
    } else {
        api_channel
    }
}

/// MFA 通过后的正式登录：原验证码 + retry_tag + 密文 sms_code_key；
/// cookie 带 registered 响应下发的新 passport_mfa_token。
pub async fn mfa_relogin(
    env: &ApiEnv,
    mobile: &str,
    code: &str,
    ctx: &MfaContext,
) -> AppResult<LoginOutcome> {
    let env = with_mfa_cookie(env, &ctx.mfa_token);
    sms_login(
        &env,
        mobile,
        code,
        Some((ctx.retry_tag.as_str(), ctx.sms_code_key.as_str())),
    )
    .await
}

/// 把 `passport_mfa_token` 覆盖进 env 的 Cookie（已有同名字段则替换，
/// 没有/无 Cookie 则追加）。MFA 会话绑定全靠它。
fn with_mfa_cookie(env: &ApiEnv, token: &str) -> ApiEnv {
    let mut next = env.clone();
    if token.is_empty() {
        return next;
    }
    let base = next.cookie.take().unwrap_or_default();
    let kept: Vec<&str> = base
        .split("; ")
        .filter(|kv| !kv.is_empty() && !kv.starts_with("passport_mfa_token="))
        .collect();
    let mut parts: Vec<String> = kept.iter().map(|s| s.to_string()).collect();
    parts.push(format!("passport_mfa_token={token}"));
    next.cookie = Some(parts.join("; "));
    next
}

/// MFA 上行短信验证轮询（单次调用，前端 3 秒间隔重发）。body 是
/// form-urlencoded 的 MFA 上下文（`biz_params` 为 JSON 串）；成功判据是 `data.registered==true`。
pub async fn upsms_verify(ctx: &MfaContext, env: &ApiEnv) -> AppResult<UpsmsState> {
    let biz = serde_json::json!({
        "passport_mfa_retry_tag": ctx.retry_tag,
        "sms_code_key": ctx.sms_code_key,
    });
    let pairs: Vec<(String, String)> = [
        ("biz_params", biz.to_string()),
        ("copywriting_key", ctx.copywriting_key.clone()),
        ("encrypt_uid", ctx.encrypt_uid.clone()),
        ("ies_safety_diversion_tag", ctx.diversion_tag.clone()),
        ("new_authn_sdk_log_id", ctx.log_id.clone()),
        ("new_authn_sdk_verify_reason", ctx.verify_reason.clone()),
        ("new_authn_sdk_verify_scene", ctx.verify_scene.clone()),
        // 客户端常量（官方原样，服务端不校验版本）
        ("new_authn_sdk_version", "1.1.31".to_string()),
        ("request_tag_from", "h5".to_string()),
        ("verify_reason", ctx.verify_reason.clone()),
        ("verify_scene", ctx.verify_scene.clone()),
        // 两个空值字段也要在场（官方原样对齐，服务端可能参与形态校验）
        ("new_verify_flow", String::new()),
        ("verify_ticket", String::new()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let body = form_urlencoded(&pairs);

    // MFA 会话绑定 cookie（缺它服务端永远回 1045）
    let env = with_mfa_cookie(env, &ctx.mfa_token);
    let q = passport_sdk_query(false);
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/upsms/verify/",
        Some(body.into_bytes()),
        &q,
        &[("content-type".into(), FORM_CONTENT_TYPE.into())],
        &env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&resp.bytes)
        .map_err(|e| AppError::Auth(format!("upsms/verify 响应不是 JSON: {e}")))?;
    interpret_upsms_response(&v, &resp.set_cookies, &ctx.mfa_token)
}

/// 解读 upsms/verify 的响应（纯函数，[`upsms_verify`] 的判据集中在此，
/// 便于离线测试真实路径）。
fn interpret_upsms_response(
    v: &Value,
    set_cookies: &[String],
    ctx_mfa_token: &str,
) -> AppResult<UpsmsState> {
    let code = error_code_of(v);
    match code {
        0 => {
            let ticket = v
                .pointer("/data/ticket")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if v.pointer("/data/registered")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                // registered 响应 Set-Cookie 新 token，紧随其后的 mfa_relogin 必须带它
                let mfa_token = extract_cookie_pairs(set_cookies)
                    .split("; ")
                    .find(|kv| kv.starts_with("passport_mfa_token="))
                    .map(|kv| kv["passport_mfa_token=".len()..].to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| ctx_mfa_token.to_string());
                Ok(UpsmsState::Registered { ticket, mfa_token })
            } else {
                // code==0 但 registered 缺失：按等待处理（形态未见过，防御）
                Ok(UpsmsState::Waiting)
            }
        }
        // 1045：仍在等待用户回复短信
        1045 => Ok(UpsmsState::Waiting),
        _ => Err(AppError::Auth(format!(
            "MFA 验证失败 {}: {}",
            code,
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_stub_upsms_channel_only() {
        assert_eq!(real_upsms_channel("9515211003"), "10691859839103");
        assert_eq!(real_upsms_channel("10691859839103"), "10691859839103");
        assert_eq!(real_upsms_channel("1069000000001"), "1069000000001");
        assert_eq!(real_upsms_channel(""), "");
    }

    #[test]
    fn interprets_upsms_response_states() {
        let waiting = serde_json::json!({"error_code": 1045, "message": "waiting"});
        assert!(matches!(
            interpret_upsms_response(&waiting, &[], "old").unwrap(),
            UpsmsState::Waiting
        ));

        let registered = serde_json::json!({
            "data": { "registered": true, "ticket": "tk" }
        });
        let cookies = vec!["passport_mfa_token=new-tok; Path=/; HttpOnly".to_string()];
        match interpret_upsms_response(&registered, &cookies, "old").unwrap() {
            UpsmsState::Registered { ticket, mfa_token } => {
                assert_eq!(ticket, "tk");
                assert_eq!(mfa_token, "new-tok", "要吃 Set-Cookie 下发的新 token");
            }
            other => panic!("期望 Registered，实际 {other:?}"),
        }

        let ambiguous = serde_json::json!({"data": {}});
        assert!(matches!(
            interpret_upsms_response(&ambiguous, &[], "old").unwrap(),
            UpsmsState::Waiting
        ));

        let rejected = serde_json::json!({"data": {"error_code": 1003}, "message": "手机号错误"});
        let err = interpret_upsms_response(&rejected, &[], "old").unwrap_err();
        assert!(err.to_string().contains("1003"));
    }

    #[test]
    fn merges_mfa_cookie_into_env() {
        let mk = |cookie: Option<&str>| ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            device: hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid()),
            cookie: cookie.map(str::to_string),
            x_tt_token: None,
        };
        let env = with_mfa_cookie(&mk(Some("a=1; passport_mfa_token=old; b=2")), "new");
        assert_eq!(
            env.cookie.as_deref(),
            Some("a=1; b=2; passport_mfa_token=new")
        );
        let env = with_mfa_cookie(&mk(None), "t1");
        assert_eq!(env.cookie.as_deref(), Some("passport_mfa_token=t1"));
        let env = with_mfa_cookie(&mk(Some("a=1")), "");
        assert_eq!(env.cookie.as_deref(), Some("a=1"), "空 token 不动 cookie");
    }
}
