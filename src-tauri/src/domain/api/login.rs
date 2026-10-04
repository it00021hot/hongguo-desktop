//! 短信登录（passport 系，`novel.snssdk.com` 同域）。
//!
//! ## 流程（hgplayer 逆向实证）
//!
//! ```text
//! POST /passport/mobile/send_code/v1/   mobile 需 XOR(0x05) hex 编码
//! POST /passport/mobile/sms_login/      mobile 同上 + code；MFA 时带上
//!                                       passport_mfa_retry_tag + sms_code_key
//! POST /passport/upsms/verify/          MFA 上行短信验证轮询，
//!                                       error_code=1045 表示仍在等待
//! GET  /reading/user/info/v1/           登录后读取当前用户信息
//! ```
//!
//! 登录成功的会话在 **Set-Cookie**（`sessionid` / `x-tt-token` 等），
//! 洗成 `k=v; k=v` 后附加到业务请求（不参与签名）。风控提示：账号与
//! 设备永久绑定，换设备或重复注册容易触发风控。

use serde_json::Value;

use super::client::{api_call_full_response, ApiEnv};
use crate::error::{AppError, AppResult};

/// passport 与业务同域（hgplayer 实测基址 `https://novel.snssdk.com`）。
pub const PASSPORT_ORIGIN: &str = "https://novel.snssdk.com";

/// 手机号编码：每个 ASCII 字节 XOR 0x05 后转小写 hex。
pub fn encode_mobile(mobile: &str) -> String {
    let mut out = String::with_capacity(mobile.len() * 2);
    for b in mobile.bytes() {
        out.push_str(&format!("{:02x}", b ^ 0x05));
    }
    out
}

/// passport 公共 query：mix_mode=0（mobile 为 XOR 密文）。
fn passport_query(mobile_enc: &str) -> Vec<(String, String)> {
    vec![
        ("mobile".into(), mobile_enc.into()),
        // mix_mode：0 = mobile 是密文，1 = 明文（fqnovel passport 惯例）
        ("mix_mode".into(), "0".into()),
    ]
}

/// 把 Set-Cookie 原始行洗成 `k=v; k=v`（剥掉 Path/Domain/Expires 等属性）。
pub fn extract_cookie_pairs(set_cookies: &[String]) -> String {
    let mut pairs = Vec::new();
    for line in set_cookies {
        if let Some(kv) = line.split(';').next() {
            let kv = kv.trim();
            if !kv.is_empty() && kv.contains('=') {
                pairs.push(kv.to_string());
            }
        }
    }
    pairs.join("; ")
}

/// 短信登录 / MFA 的结构化结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoginOutcome {
    /// 登录成功：会话 cookie + 服务端返回的用户信息。
    Success {
        /// `k=v; k=v`（可直接放进 `ApiEnv::cookie`）
        cookies: String,
        user: PassportUser,
    },
    /// 需要短信上行 MFA 二次验证：带着上下文轮询 [`upsms_verify`]。
    Mfa {
        retry_tag: String,
        sms_code_key: String,
        /// 服务端给的验证提示（如「回复 XX 到 1xx」）
        tips: String,
    },
}

/// 登录响应里的用户信息（字段按 fqnovel passport 惯例宽松解析）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassportUser {
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub mobile: String,
}

/// MFA 上行短信验证的轮询状态。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum UpsmsState {
    /// error_code=1045：用户还没回复短信，继续轮询
    Waiting,
    /// 验证通过（携带会话）
    Success { cookies: String, user: PassportUser },
}

/// 发送短信验证码。返回服务端 message（成功一般是「验证码已发送」类文案）。
pub async fn send_sms_code(env: &ApiEnv, mobile: &str) -> AppResult<String> {
    let q = passport_query(&encode_mobile(mobile));
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/mobile/send_code/v1/",
        Some(Vec::new()),
        &q,
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&resp.bytes)
        .map_err(|e| AppError::Media(format!("send_code 响应不是 JSON: {e}")))?;
    check_error(&v, "发送验证码失败")?;
    Ok(v
        .get("data")
        .and_then(|d| d.get("description"))
        .or_else(|| v.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("验证码已发送")
        .to_string())
}

/// 短信验证码登录。MFA 场景传入 [`LoginOutcome::Mfa`] 的上下文重试。
pub async fn sms_login(
    env: &ApiEnv,
    mobile: &str,
    code: &str,
    mfa: Option<(&str, &str)>, // (passport_mfa_retry_tag, sms_code_key)
) -> AppResult<LoginOutcome> {
    let mut q = passport_query(&encode_mobile(mobile));
    q.push(("code".into(), code.into()));
    if let Some((tag, key)) = mfa {
        q.push(("passport_mfa_retry_tag".into(), tag.into()));
        q.push(("sms_code_key".into(), key.into()));
    }
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/mobile/sms_login/",
        Some(Vec::new()),
        &q,
        &[],
        env,
    )
    .await?;
    parse_login_response(&resp.bytes, &resp.set_cookies)
}

/// MFA 上行短信验证轮询。验证通过时响应带会话 cookie。
pub async fn upsms_verify(
    env: &ApiEnv,
    retry_tag: &str,
    sms_code_key: &str,
) -> AppResult<UpsmsState> {
    let q = vec![
        ("passport_mfa_retry_tag".into(), retry_tag.into()),
        ("sms_code_key".into(), sms_code_key.into()),
    ];
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/upsms/verify/",
        Some(Vec::new()),
        &q,
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&resp.bytes)
        .map_err(|e| AppError::Media(format!("upsms/verify 响应不是 JSON: {e}")))?;
    let code = error_code_of(&v);
    match code {
        0 => {
            let cookies = extract_cookie_pairs(&resp.set_cookies);
            if cookies.is_empty() {
                return Err(AppError::Media(
                    "upsms/verify 通过但未返回会话 cookie".into(),
                ));
            }
            let user = parse_user(&v);
            Ok(UpsmsState::Success { cookies, user })
        }
        // 1045：仍在等待用户回复短信（hgplayer 注释实证）
        1045 => Ok(UpsmsState::Waiting),
        _ => Err(AppError::Media(format!(
            "MFA 验证失败 {}: {}",
            code,
            v.get("message").and_then(Value::as_str).unwrap_or("未知错误")
        ))),
    }
}

/// 登录后的当前用户信息（`/reading/user/info/v1/`，需要 cookie 环境）。
pub async fn user_info(env: &ApiEnv) -> AppResult<PassportUser> {
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/reading/user/info/v1/",
        None,
        &[],
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&resp.bytes)
        .map_err(|e| AppError::Media(format!("user/info 响应不是 JSON: {e}")))?;
    check_error(&v, "读取用户信息失败")?;
    Ok(parse_user(&v))
}

/// 解析登录响应：成功取 Set-Cookie + 用户信息；MFA 特征字段在场则走 MFA 分支。
fn parse_login_response(bytes: &[u8], set_cookies: &[String]) -> AppResult<LoginOutcome> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Media(format!("sms_login 响应不是 JSON: {e}")))?;

    // MFA 特征（hgplayer 参数表/注释实证）：这些字段只在 MFA 场景出现
    let tag = str_field(&v, &["passport_mfa_retry_tag", "data.passport_mfa_retry_tag"]);
    let key = str_field(&v, &["sms_code_key", "data.sms_code_key"]);
    if let (Some(tag), Some(key)) = (tag, key) {
        return Ok(LoginOutcome::Mfa {
            retry_tag: tag,
            sms_code_key: key,
            tips: str_field(&v, &["new_authn_sdk_verify_reason", "data.new_authn_sdk_verify_reason"])
                .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default(),
        });
    }

    check_error(&v, "登录失败")?;

    let cookies = extract_cookie_pairs(set_cookies);
    if cookies.is_empty() {
        return Err(AppError::Media("登录成功但未返回会话 cookie".into()));
    }
    Ok(LoginOutcome::Success {
        cookies,
        user: parse_user(&v),
    })
}

/// passport 惯例：`error_code` 在 `data` 里（实测 send_code 拒绝时
/// `{"data":{"error_code":1003,"description":"手机号错误…"},"message":"error"}`），
/// 顶层只有 `message`。两个位置都兜，错误文案优先 `data.description`。
fn error_code_of(v: &Value) -> i64 {
    v.get("data")
        .and_then(|d| d.get("error_code"))
        .and_then(Value::as_i64)
        .or_else(|| v.get("error_code").and_then(Value::as_i64))
        .unwrap_or(0)
}

fn check_error(v: &Value, prefix: &str) -> AppResult<()> {
    let data = v.get("data");
    let code = error_code_of(v);
    if code != 0 {
        let msg = data
            .and_then(|d| d.get("description"))
            .or_else(|| data.and_then(|d| d.get("message")))
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(AppError::Media(format!("{prefix} {code}: {msg}")));
    }
    Ok(())
}

/// 按候选路径宽松取字符串字段。
fn str_field(v: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| {
        let mut cur = v;
        for seg in p.split('.') {
            cur = cur.get(seg)?;
        }
        cur.as_str().map(str::to_string).filter(|s| !s.is_empty())
    })
}

/// 宽松解析用户信息：fqnovel passport 的 data 结构在 sms_login 与
/// user_info 两个端点上字段名略有出入（name/user_name），都兜住。
fn parse_user(v: &Value) -> PassportUser {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let inner = data.get("user_info").cloned().unwrap_or(data);
    PassportUser {
        user_id: str_field(&inner, &["user_id", "userId", "uid"])
            .or_else(|| {
                inner
                    .get("user_id")
                    .and_then(Value::as_i64)
                    .map(|n| n.to_string())
            })
            .unwrap_or_default(),
        name: str_field(&inner, &["name", "user_name", "username", "nick_name"])
            .unwrap_or_default(),
        mobile: str_field(&inner, &["mobile"]).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安全探测：无效号段（100 开头不是有效手机段）发码——服务端应拒绝
    /// 号码而不下发短信。验证「域名/签名/query/XOR 编码/mix_mode」整条
    /// 链路：若返回的是「手机号相关错误」说明链路通；「参数错误」则编码
    /// 或 mix_mode 有误。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例（不下发短信）"]
    async fn probe_send_code_invalid_number() {
        use super::super::client::api_call_full_response;
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let q = passport_query(&encode_mobile("10000000000"));
        let resp = api_call_full_response(
            PASSPORT_ORIGIN,
            "/passport/mobile/send_code/v1/",
            Some(Vec::new()),
            &q,
            &[],
            &env,
        )
        .await
        .expect("请求应可达");
        let text = String::from_utf8_lossy(&resp.bytes);
        println!("[probe] body: {}", &text[..text.len().min(500)]);
    }

    /// 安全探测：sms_login 用无效号段 + 假验证码——服务端在号码校验
    /// 就会拒绝（1003），不涉及真实短信/会话。验证 sms_login 的
    /// query 形态（mobile/code/mix_mode）被服务端接受。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_sms_login_invalid_number() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        match sms_login(&env, "10000000000", "123456", None).await {
            Ok(outcome) => println!("[probe-sms] 意外成功: {outcome:?}"),
            Err(e) => println!("[probe-sms] 拒绝: {e}"),
        }
    }

    /// XOR(0x05) hex：'1'=0x31→34，'3'=0x33→36，'8'=0x38→3d，'0'=0x30→35
    #[test]
    fn encodes_mobile_with_xor05() {
        assert_eq!(encode_mobile("13800138000"), "34363d353534363d353535");
    }

    /// Set-Cookie 清洗：剥属性、保序、跳过非法行。
    #[test]
    fn extracts_cookie_pairs() {
        let lines = vec![
            "sessionid=abc123; Path=/; Domain=.snssdk.com; HttpOnly".into(),
            "x-tt-token=xx|yy; Path=/".into(),
            "badline-no-equals".into(),
        ];
        assert_eq!(
            extract_cookie_pairs(&lines),
            "sessionid=abc123; x-tt-token=xx|yy"
        );
    }

    /// 成功响应：cookie + 宽松用户字段。
    #[test]
    fn parses_success_login_response() {
        let body = r#"{"error_code":0,"message":"success","data":{"user_id":7100012345678,"name":"红果用户","mobile":"138****0000"}}"#.as_bytes();
        let cookies = vec!["sessionid=s1; Path=/".to_string()];
        match parse_login_response(body, &cookies) {
            Ok(LoginOutcome::Success { cookies, user }) => {
                assert_eq!(cookies, "sessionid=s1");
                assert_eq!(user.user_id, "7100012345678");
                assert_eq!(user.name, "红果用户");
            }
            other => panic!("应解析为 Success，实际 {other:?}"),
        }
    }

    /// MFA 分支：retry_tag + sms_code_key 在场即触发。
    #[test]
    fn detects_mfa_response() {
        let body = r#"{"error_code":4016,"message":"需要二次验证","passport_mfa_retry_tag":"tag-1","sms_code_key":"key-9","new_authn_sdk_verify_reason":"回复短信完成验证"}"#.as_bytes();
        match parse_login_response(body, &[]) {
            Ok(LoginOutcome::Mfa { retry_tag, sms_code_key, tips }) => {
                assert_eq!(retry_tag, "tag-1");
                assert_eq!(sms_code_key, "key-9");
                assert!(tips.contains("回复短信"));
            }
            other => panic!("应解析为 Mfa，实际 {other:?}"),
        }
    }

    /// 常规失败（顶层 error_code 形态）。
    #[test]
    fn surfaces_error_code() {
        let body = r#"{"error_code":1201,"message":"验证码错误"}"#.as_bytes();
        let err = parse_login_response(body, &[]).unwrap_err();
        assert!(err.to_string().contains("1201"));
        assert!(err.to_string().contains("验证码错误"));
    }

    /// 真机抓包形态（2026-10-04 send_code 无效号段探测）：error_code
    /// 嵌在 data 里、description 带原因。锁住嵌套解析。
    #[test]
    fn surfaces_nested_error_code() {
        let body = r#"{"data":{"captcha":"","desc_url":"","description":"手机号错误，请重新填写","error_code":1003,"mobile_ticket":""},"message":"error"}"#.as_bytes();
        let v: Value = serde_json::from_slice(body).unwrap();
        let err = (|| {
            check_error(&v, "发送验证码失败")?;
            Ok::<(), crate::error::AppError>(())
        })();
        let err = err.unwrap_err();
        assert!(err.to_string().contains("1003"));
        assert!(err.to_string().contains("手机号错误"));
    }

    /// upsms 1045 等待 / 0 成功。
    #[test]
    fn parses_upsms_states() {
        let waiting: Value = serde_json::from_str(r#"{"error_code":1045,"message":"waiting"}"#).unwrap();
        assert_eq!(waiting.get("error_code").and_then(Value::as_i64), Some(1045));
    }
}
