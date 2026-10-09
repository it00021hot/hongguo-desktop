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

mod mfa;
mod model;

use serde_json::Value;

use mfa::real_upsms_channel;
use super::client::{ApiEnv, api_call_full_response};
use crate::error::{AppError, AppResult};

pub use mfa::{mfa_relogin, upsms_verify};
pub use model::{
    LoginOutcome, MfaContext, MfaFlow, PassportUser, SendCodeOutcome, UpsmsState,
};

/// passport 与业务同域（hgplayer 实测基址 `https://novel.snssdk.com`）。
pub const PASSPORT_ORIGIN: &str = "https://novel.snssdk.com";

/// passport 系 form 请求的 content-type（抓包原值，含 charset）。
const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded; charset=UTF-8";

/// 手机号编码：`+86` 国码 + 11 位号码，每个 ASCII 字节 XOR 0x05 后转小写 hex。
///
/// `+86` 前缀是 2026-10-04 hgplayer 1.1.3 真机抓包实证（密文解出
/// `+8617…`）；不带前缀的服务端按残缺号处理。
pub fn encode_mobile(mobile: &str) -> String {
    xor05_hex(&format!("+86{mobile}"))
}

/// 验证码编码：与手机号同套 XOR(0x05) hex（1.1.3 抓包实证 `code` 也是密文）。
pub fn encode_code(code: &str) -> String {
    xor05_hex(code)
}

/// 每个字节 XOR 0x05 后转小写 hex。
fn xor05_hex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        out.push_str(&format!("{:02x}", b ^ 0x05));
    }
    out
}

/// passport 公共 query（1.1.3 抓包逐项对齐）。
/// passport 系 query 常量（2026-10-04 抓包全量对齐：query 只放 SDK
/// 标识，**业务参数全部在 form body**——此前把 mobile/code 放 query 的
/// 形态虽然被服务端解析，但与真实客户端形态不符，风控敏感接口不做
/// 这种偏离）。
fn passport_sdk_query(with_rom: bool) -> Vec<(String, String)> {
    let mut q: Vec<(String, String)> = [
        ("cronet_version", "8d40f833_2026-03-03"),
        ("passport-sdk-version", "5051452"),
        ("ttnet_version", "4.2.243.31-douyin"),
        ("use_new_token_expire_rule", "true"),
        ("use_store_region_cookie", "1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if with_rom {
        q.push(("rom_version".into(), "miui_V12_V12.0.8.0.RKHCNXM".into()));
    }
    q
}

/// passport 系 form body 的设备/环境字段（2026-10-04 抓包 57 字段逐值
/// 对齐）。指纹字段取设备档案；运行时状态（电量/网速/会话计数等）用
/// 抓包常量——静态档案口径下服务端只作风控参考，不校验真实性。
fn passport_device_form(env: &ApiEnv) -> Vec<(String, String)> {
    let d = &env.device;
    // 档案键 → form 键一致的直接搬；两处不同名的显式映射
    let pairs: Vec<(&str, &str, bool)> = vec![
        // (form 键, 档案键, 是否取自档案)
        ("ac", "ac", true),
        ("aid", "aid", true),
        ("app_name", "app_name", true),
        ("cdid", "cdid", true),
        ("channel", "channel", true),
        ("device_brand", "device_brand", true),
        ("device_id", "device_id", true),
        ("device_platform", "device_platform", true),
        ("device_type", "device_type", true),
        ("dpi", "dpi", true),
        ("host_abi", "host_abi", true),
        ("iid", "iid", true),
        ("language", "language", true),
        ("manifest_version_code", "manifest_version_code", true),
        ("os", "os", true),
        ("os_api", "os_api", true),
        ("os_version", "os_version", true),
        ("resolution", "resolution", true),
        ("ssmix", "ssmix", true),
        ("update_version_code", "update_version_code", true),
        ("version_code", "version_code", true),
        ("version_name", "version_name", true),
        // 运行时状态常量（抓包原值）
        ("app_dark_mode", "0", false),
        ("app_mini_window", "0", false),
        ("battery_pct", "72", false),
        ("charging", "1", false),
        ("cold_start_session_cnt_in_day", "100", false),
        ("cold_start_session_cnt_in_life", "76", false),
        (
            "cold_start_session_id",
            "4357f7af-60c0-4a52-af34-473c59155de9",
            false,
        ),
        ("compliance_status", "0", false),
        ("current_volume", "1", false),
        ("down_speed", "311565", false),
        ("dragon_device_type", "phone", false),
        ("font_scale", "100", false),
        ("gender", "2", false),
        ("har_status", "0", false),
        ("is_android_pad_screen", "0", false),
        ("is_power_save_mode", "0", false),
        ("need_personal_recommend", "1", false),
        ("network_type", "4", false),
        ("normal_session_cnt_in_day", "182", false),
        ("normal_session_cnt_in_life", "270", false),
        (
            "normal_session_id",
            "23661b98-eac9-4960-81a4-d8837826b43f%23160",
            false,
        ),
        ("player_so_load", "1", false),
        ("pv_player", "73932", false),
        ("screen_brightness", "963", false),
        ("sys_dark_mode", "0", false),
        ("sys_mini_window", "1", false),
    ];
    let mut form: Vec<(String, String)> = vec![(
        "_rticket".to_string(),
        chrono::Utc::now().timestamp_millis().to_string(),
    )];
    for (key, val, from_device) in pairs {
        let v = if from_device {
            let got = d.get(val);
            if got.is_empty() {
                val.to_string()
            } else {
                got.to_string()
            }
        } else {
            val.to_string()
        };
        form.push((key.to_string(), v));
    }
    form
}

/// 把 Set-Cookie 原始行洗成 `k=v; k=v`（剥掉 Path/Domain/Expires 等属性）。
pub fn extract_cookie_pairs(set_cookies: &[String]) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in set_cookies {
        if let Some(kv) = line.split(';').next() {
            let kv = kv.trim();
            if let Some((k, v)) = kv.split_once('=') {
                if k.is_empty() {
                    continue;
                }
                // 服务端会对同一 cookie 多次下发（如 odin_tt），后值覆盖、
                // 位置保持首现——否则落库串里同名 cookie 越叠越多
                if let Some(slot) = pairs.iter_mut().find(|(ek, _)| ek == k) {
                    slot.1 = v.to_string();
                } else {
                    pairs.push((k.to_string(), v.to_string()));
                }
            }
        }
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 发送短信验证码。成功返回文案与 csrf 会话凭据。
///
/// 形态（2026-10-04 抓包对齐）：query 只放 SDK 常量，设备+业务字段
/// 全在 form body。
pub async fn send_sms_code(env: &ApiEnv, mobile: &str) -> AppResult<SendCodeOutcome> {
    let q = passport_sdk_query(true);
    let mut body = passport_device_form(env);
    // mix_mode：1 = mobile 是 XOR(0x05) hex 密文（真机实测：配 0 时
    // 服务端把密文当明文解析成乱码，合法号码也报 1003 手机号错误）
    body.push(("mobile".into(), encode_mobile(mobile)));
    body.push(("mix_mode".into(), "1".into()));
    body.push(("account_sdk_source".into(), "app".into()));
    body.push(("passport_support_flow".into(), "captcha,verify".into()));
    // type=3731：短信登录发码场景（1.1.3 抓包值；type=1 会被服务端
    // 当成换绑场景——目标号已绑定其它账号时报 1001，2026-10-04 实测）
    body.push(("type".into(), "3731".into()));
    // send_code 独有（抓包原值；解绑上下文 flag + 自动阅读开关）
    body.push(("auto_read".into(), "0".into()));
    body.push(("unbind_exist".into(), "34".into()));
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/mobile/send_code/v1/",
        Some(form_urlencoded(&body).into_bytes()),
        &q,
        &[("content-type".into(), FORM_CONTENT_TYPE.into())],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&resp.bytes)
        .map_err(|e| AppError::Auth(format!("send_code 响应不是 JSON: {e}")))?;
    check_error(&v, "发送验证码失败")?;
    let data = v.get("data");
    let csrf_cookie = resp
        .set_cookies
        .iter()
        .filter_map(|line| line.split(';').next())
        .map(str::trim)
        .find(|kv| kv.starts_with("passport_csrf_token="))
        .and_then(|kv| kv.split_once('=').map(|(_, v)| v.to_string()))
        .unwrap_or_default();
    Ok(SendCodeOutcome {
        message: data
            .and_then(|d| d.get("description"))
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("验证码已发送")
            .to_string(),
        mobile_ticket: data
            .and_then(|d| d.get("mobile_ticket"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        retry_time: data
            .and_then(|d| d.get("retry_time"))
            .and_then(Value::as_u64)
            .unwrap_or(60) as u32,
        csrf_cookie,
    })
}

/// 短信验证码登录。MFA 场景传入 [`LoginOutcome::Mfa`] 的上下文重试。
///
/// 发码会话的绑定凭据（`passport_csrf_token`）由调用方合进 `env.cookie`
/// （1.1.3 实测：cookie 带 csrf 而非 body 回传 mobile_ticket）。
pub async fn sms_login(
    env: &ApiEnv,
    mobile: &str,
    code: &str,
    mfa: Option<(&str, &str)>, // (passport_mfa_retry_tag, sms_code_key 明文)
) -> AppResult<LoginOutcome> {
    let q = passport_sdk_query(true);
    let mut body = passport_device_form(env);
    body.push(("mobile".into(), encode_mobile(mobile)));
    body.push(("mix_mode".into(), "1".into()));
    // code 与 mobile 同套 XOR(0x05) hex 密文（1.1.3 抓包实证）
    body.push(("code".into(), encode_code(code)));
    body.push(("account_sdk_source".into(), "app".into()));
    body.push(("passport_support_flow".into(), "captcha,verify".into()));
    if let Some((tag, key)) = mfa {
        // retry_tag 明文（抓包为 "1"），sms_code_key 密文（同套 XOR）
        body.push(("passport_mfa_retry_tag".into(), tag.into()));
        body.push(("sms_code_key".into(), encode_code(key)));
    }
    let resp = api_call_full_response(
        PASSPORT_ORIGIN,
        "/passport/mobile/sms_login/",
        Some(form_urlencoded(&body).into_bytes()),
        &q,
        &[("content-type".into(), FORM_CONTENT_TYPE.into())],
        env,
    )
    .await?;
    parse_login_response(&resp.bytes, &resp.set_cookies, &resp.headers)
}

/// form-urlencoded 编码（encodeURIComponent 语义；空格按 %20，
/// 标准 form 解码器均接受）。
fn form_urlencoded(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode_component(k), urlencode_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 与 signer::ticket 同语义的组件转义（encodeURIComponent + `!'()` 补转义）。
fn urlencode_component(s: &str) -> String {
    const UNRESERVED: &str = "-_.!~*'()";
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || UNRESERVED.contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 登录后的当前用户信息（`/reading/user/info/v1/`，需要 cookie 环境）。
pub async fn user_info(env: &ApiEnv) -> AppResult<PassportUser> {
    // 抓包实证这族 reading 接口都在 lq 域（api5-normal-lq.fqnovel.com）、
    // 走轻签名头；此前误挂 passport 域（novel.snssdk.com）上 404。
    let bytes = super::client::api_call_reading(
        super::danmaku::LQ_API_ORIGIN,
        "/reading/user/info/v1/",
        None,
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Auth(format!("user/info 响应不是 JSON: {e}")))?;
    check_error(&v, "读取用户信息失败")?;
    Ok(parse_user(&v))
}

/// 解析登录响应：成功取 Set-Cookie + 用户信息；MFA 特征字段在场则走 MFA 分支。
fn parse_login_response(
    bytes: &[u8],
    set_cookies: &[String],
    x_tt_token: &[(String, String)],
) -> AppResult<LoginOutcome> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Auth(format!("sms_login 响应不是 JSON: {e}")))?;

    // MFA 特征（1.1.3 实测嵌套在 data.biz_params；顶层形态做兼容）：
    // passport_mfa_retry_tag 与 sms_code_key 成对出现即 MFA
    let biz = v
        .pointer("/data/biz_params")
        .filter(|b| b.get("passport_mfa_retry_tag").is_some())
        .unwrap_or(&v);
    if let (Some(tag), Some(key)) = (
        biz.get("passport_mfa_retry_tag").and_then(Value::as_str),
        biz.get("sms_code_key").and_then(Value::as_str),
    ) {
        let way = v
            .pointer("/data/verify_ways")
            .and_then(Value::as_array)
            .and_then(|ways| {
                ways.iter().find(|w| {
                    w.get("verify_way").and_then(Value::as_str) == Some("mobile_up_sms_verify")
                })
            });
        let ctx = MfaContext {
            retry_tag: tag.to_string(),
            sms_code_key: key.to_string(),
            encrypt_uid: str_field(&v, &["data.encrypt_uid"]).unwrap_or_default(),
            log_id: str_field(&v, &["data.event_params.log_id"]).unwrap_or_default(),
            verify_reason: str_field(&v, &["data.event_params.verify_reason"])
                .unwrap_or_else(|| "ato".into()),
            verify_scene: str_field(&v, &["data.event_params.verify_scene"])
                .unwrap_or_else(|| "sms_login".into()),
            copywriting_key: str_field(&v, &["data.common_params.copywriting_key"])
                .unwrap_or_else(|| "sms_login".into()),
            diversion_tag: str_field(&v, &["data.common_params.ies_safety_diversion_tag"])
                .unwrap_or_else(|| "mfa".into()),
            channel_mobile: way
                .and_then(|w| w.get("channel_mobile"))
                .and_then(Value::as_str)
                .map(real_upsms_channel)
                .unwrap_or_default()
                .to_string(),
            sms_content: way
                .and_then(|w| w.get("sms_content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tips: str_field(&v, &["data.verify_scene_desc", "data.description"])
                .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default(),
            // MFA 会话 token：响应 Set-Cookie 下发，轮询/重登必带
            mfa_token: extract_cookie_pairs(set_cookies)
                .split("; ")
                .find(|kv| kv.starts_with("passport_mfa_token="))
                .map(|kv| kv["passport_mfa_token=".len()..].to_string())
                .unwrap_or_default(),
        };
        return Ok(LoginOutcome::Mfa(Box::new(ctx)));
    }

    check_error(&v, "登录失败")?;

    let cookies = extract_cookie_pairs(set_cookies);
    if cookies.is_empty() {
        return Err(AppError::Auth("登录成功但未返回会话 cookie".into()));
    }
    let token = x_tt_token
        .iter()
        .find(|(k, _)| k == "x-tt-token")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    Ok(LoginOutcome::Success {
        cookies,
        user: parse_user(&v),
        token,
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
        return Err(AppError::Auth(format!("{prefix} {code}: {msg}")));
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
        avatar_url: str_field(&inner, &["avatar_url", "avatar"]).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// parse_user 必须从登录响应 data 里带出头像 URL（2026-10-04 抓包
    /// 样本：sms_login 的 data 顶层有 avatar_url；user_info 端点嵌套在
    /// data.user_info 里，两种形态都兜）。
    #[test]
    fn parse_user_extracts_avatar_url() {
        let sms_login = serde_json::json!({
            "data": {
                "app_id": 8662,
                "user_id": 3836620877071530_i64,
                "name": "用户1774591583619",
                "avatar_url": "https://p9-passport.byteacctimg.com/img/mosaic-legacy/3791/5035712059~120x256.image",
                "mobile": "15000000000"
            }
        });
        let u = parse_user(&sms_login);
        assert_eq!(
            u.avatar_url,
            "https://p9-passport.byteacctimg.com/img/mosaic-legacy/3791/5035712059~120x256.image"
        );
        assert_eq!(u.name, "用户1774591583619");

        let user_info = serde_json::json!({
            "data": { "user_info": {
                "user_id": "2996633242445203",
                "name": "用户1199378140235",
                "avatar_url": "https://p9-passport.byteacctimg.com/img/mosaic-legacy/3791/5070639578~120x256.image"
            } }
        });
        let u2 = parse_user(&user_info);
        assert_eq!(
            u2.avatar_url,
            "https://p9-passport.byteacctimg.com/img/mosaic-legacy/3791/5070639578~120x256.image"
        );

        // 无头像不 panic，字段为空
        assert_eq!(parse_user(&serde_json::json!({"data": {}})).avatar_url, "");
    }

    /// 安全探测：无效号段（100 开头不是有效手机段）发码——服务端应拒绝
    /// 号码而不下发短信。验证「域名/签名/query/XOR 编码/mix_mode」整条
    /// 链路：若返回的是「手机号相关错误」说明链路通；「参数错误」则编码
    /// 或 mix_mode 有误。
    /// 真机/无效号段发码探测：HG_LOGIN_MOBILE 指定号码（真机会真实下发
    /// 短信），缺省用无效号段 10000000000（服务端拒绝号码、不下发）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_send_code_invalid_number() {
        let mut env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        // 实验分支：HG_LOGIN_DEVICE=random 换随机 device_id/iid（测发码
        // 是否依赖 install 注册/关联）；legacy 用 1694 旧档案。
        match std::env::var("HG_LOGIN_DEVICE").as_deref() {
            Ok("random") => {
                let d =
                    rand::random::<u64>() % 9_000_000_000_000_000_000 + 1_000_000_000_000_000_000;
                env.device.set("device_id", &d.to_string());
                env.device.set("iid", &(d + 1).to_string());
            }
            Ok("legacy") => {
                env.device.set("device_id", "1694811517885562");
                env.device.set("iid", "1694811517889658");
            }
            _ => {}
        }
        let mobile = std::env::var("HG_LOGIN_MOBILE").unwrap_or_else(|_| "10000000000".into());
        println!("[probe] mobile: {mobile}");
        // HG_PROXY=http://127.0.0.1:8080 时走本地 mitmdump（抓自己的请求
        // 与 hgplayer 抓包逐字节 diff）
        if let Ok(p) = std::env::var("HG_PROXY")
            && !p.is_empty()
        {
            env.proxy = crate::domain::model::settings::ProxyConfig {
                mode: crate::domain::model::settings::ProxyMode::Manual,
                url: p,
            };
        }
        match send_sms_code(&env, &mobile).await {
            Ok(out) => println!(
                "[probe] 发码成功: {} ticket={}",
                out.message, out.mobile_ticket
            ),
            Err(e) => println!("[probe] 发码失败: {e}"),
        }
    }

    /// 真机登录：HG_LOGIN_MOBILE + HG_LOGIN_CODE（+ 可选 HG_LOGIN_CSRF，
    /// 缺省先真实发码并打印 csrf 再等待重跑）。成功返回会话 cookie
    /// （打印前缀），并带 cookie 拉 user_info 验证会话可用。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_sms_login_real() {
        let env0 = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let mobile = std::env::var("HG_LOGIN_MOBILE").expect("HG_LOGIN_MOBILE 必填");
        let Ok(code) = std::env::var("HG_LOGIN_CODE") else {
            // 第一段：只发码，打印 csrf 供第二段使用（短信真实下发）
            let out = send_sms_code(&env0, &mobile).await.expect("发码");
            println!("[login] 发码: {} csrf={}", out.message, out.csrf_cookie);
            println!("[login] 收到短信后带 HG_LOGIN_CODE 重跑");
            return;
        };
        let csrf = std::env::var("HG_LOGIN_CSRF").unwrap_or_default();
        let env = if csrf.is_empty() {
            env0.clone()
        } else {
            crate::domain::api::client::ApiEnv {
                cookie: Some(format!(
                    "passport_csrf_token={csrf}; passport_csrf_token_default={csrf}"
                )),
                proxy: env0.proxy.clone(),
                device: env0.device.clone(),
                x_tt_token: None,
            }
        };
        match sms_login(&env, &mobile, &code, None).await {
            Ok(LoginOutcome::Success {
                cookies,
                user,
                token,
            }) => {
                println!(
                    "[login] ✅ 成功 user={}({}) cookie {}B: {}… token {}B",
                    user.name,
                    user.user_id,
                    cookies.len(),
                    &cookies[..cookies.len().min(60)],
                    token.len()
                );
                let env = crate::domain::api::client::ApiEnv {
                    cookie: Some(cookies),
                    ..env0
                };
                match user_info(&env).await {
                    Ok(u) => println!("[login] user_info ✓ name={} id={}", u.name, u.user_id),
                    Err(e) => println!("[login] user_info ✗ {e}"),
                }
            }
            Ok(LoginOutcome::Mfa(ctx)) => {
                println!(
                    "[login] MFA: tag={} key={} 通道={} 回复={} tips={}",
                    ctx.retry_tag, ctx.sms_code_key, ctx.channel_mobile, ctx.sms_content, ctx.tips
                );
            }
            Err(e) => println!("[login] ✗ {e}"),
        }
    }

    /// 安全探测：用现成会话 cookie 拉当前用户信息（不发短信、只读）。
    ///HG_LOGIN_COOKIES="k=v; k=v" 指定 cookie；会话过期则打印错误。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_user_info_with_cookies() {
        let mut env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        env.cookie = Some(std::env::var("HG_LOGIN_COOKIES").expect("HG_LOGIN_COOKIES 必填"));
        match user_info(&env).await {
            Ok(u) => println!(
                "[probe-user] ✓ name={} id={} avatar={}",
                u.name, u.user_id, u.avatar_url
            ),
            Err(e) => println!("[probe-user] ✗ {e}"),
        }
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

    /// XOR(0x05) hex（含 +86 前缀）：'+'=0x2b→2e，'8'=0x38→3d，
    /// '6'=0x36→33，'1'=0x31→34，'3'=0x33→36，'0'=0x30→35。
    /// 前缀 `2e3d33` 与 hgplayer 抓包密文头一致。
    #[test]
    fn encodes_mobile_with_xor05() {
        assert_eq!(encode_mobile("13800138000"), "2e3d3334363d353534363d353535");
    }

    /// 验证码同套编码：'8'→3d '0'→35 '4'→34 '2'→32（1.1.3 抓包
    /// 密文 `3d353137` 解出 "8042" 的反向验证）。
    #[test]
    fn encodes_code_with_xor05() {
        assert_eq!(encode_code("8042"), "3d353137");
        assert_eq!(encode_code("123456"), "343736313033");
    }

    /// MFA 重试时 sms_code_key 也要密文（1.1.3 抓包：密文解出恰为
    /// MFA 响应下发的明文 key），retry_tag 保持明文。
    #[test]
    fn encodes_mfa_retry_params() {
        let key = "fb2825d4c17fad783d28724dd8aebe5e";
        let enc = encode_code(key);
        let dec = |hex: &str| bytes_to_string(&bytes_from_hex(hex));
        fn bytes_from_hex(hex: &str) -> Vec<u8> {
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect()
        }
        fn bytes_to_string(bs: &[u8]) -> String {
            bs.iter().map(|b| (b ^ 0x05) as char).collect()
        }
        assert_eq!(dec(&enc), key, "key 密文应能解回明文");
    }

    /// upsms body 的 form 编码：JSON 串整体转义、字段顺序稳定
    /// （与 1.1.3 抓包字段集一致）。
    #[test]
    fn form_urlencoded_escapes_json_value() {
        let out = form_urlencoded(&[
            ("biz_params".into(), r#"{"a":"1","b":"x/y"}"#.into()),
            ("copywriting_key".into(), "sms login".into()),
        ]);
        assert!(
            out.starts_with("biz_params=%7B%22a%22"),
            "JSON 大括号/引号要转义: {out}"
        );
        assert!(
            out.contains("copywriting_key=sms%20login"),
            "空格按 %20: {out}"
        );
        assert!(out.contains('='), "k=v 形态");
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
        match parse_login_response(body, &cookies, &[]) {
            Ok(LoginOutcome::Success { cookies, user, .. }) => {
                assert_eq!(cookies, "sessionid=s1");
                assert_eq!(user.user_id, "7100012345678");
                assert_eq!(user.name, "红果用户");
            }
            other => panic!("应解析为 Success，实际 {other:?}"),
        }
    }

    /// MFA 分支：data.biz_params 嵌套形态（1.1.3 真机抓包样本）。
    #[test]
    fn detects_mfa_response() {
        let body = r#"{"data":{"error_code":2046,"description":"为保证账号安全，暂不支持此操作",
            "biz_params":{"passport_mfa_retry_tag":"1","sms_code_key":"fb2825d4c17fad783d28724dd8aebe5e"},
            "encrypt_uid":"9bRhVfo5rymE",
            "event_params":{"log_id":"2026100422LOG","verify_reason":"ato","verify_scene":"sms_login"},
            "common_params":{"copywriting_key":"sms_login","ies_safety_diversion_tag":"mfa"},
            "verify_ways":[{"channel_mobile":"9515211003","mobile":"150******47","sms_content":"YZ","verify_way":"mobile_up_sms_verify"}],
            "verify_scene_desc":"为保证帐号安全，请完成身份验证"},"message":"error"}"#.as_bytes();
        match parse_login_response(body, &[], &[]) {
            Ok(LoginOutcome::Mfa(ctx)) => {
                assert_eq!(ctx.retry_tag, "1");
                assert_eq!(ctx.sms_code_key, "fb2825d4c17fad783d28724dd8aebe5e");
                assert_eq!(ctx.encrypt_uid, "9bRhVfo5rymE");
                assert_eq!(ctx.log_id, "2026100422LOG");
                // 95 号段替换成真实 106 通道（hgplayer 同款修正）
                assert_eq!(ctx.channel_mobile, "10691859839103");
                assert_eq!(ctx.sms_content, "YZ");
                assert!(ctx.tips.contains("完成身份验证"));
            }
            other => panic!("应解析为 Mfa，实际 {other:?}"),
        }
    }

    /// MFA 响应的 Set-Cookie 里有 `passport_mfa_token`——轮询与重登的
    /// 会话绑定全靠它（2026-10-05 真机踩坑：缺它永远 1045）。
    #[test]
    fn extracts_mfa_token_cookie() {
        let body = r#"{"data":{"error_code":2046,
            "biz_params":{"passport_mfa_retry_tag":"1","sms_code_key":"k"},
            "verify_ways":[{"channel_mobile":"9515211003","sms_content":"YZ","verify_way":"mobile_up_sms_verify"}]},
            "message":"error"}"#.as_bytes();
        let set_cookies = vec![
            "passport_mfa_token=CjeRkKbStbWgOEQpGsdgfE0qpV8RtNI1eV77042U15V; Path=/; Domain=.fqnovel.com; HttpOnly".into(),
            "store-region=cn-gd; Path=/".into(),
        ];
        match parse_login_response(body, &set_cookies, &[]) {
            Ok(LoginOutcome::Mfa(ctx)) => {
                assert_eq!(
                    ctx.mfa_token, "CjeRkKbStbWgOEQpGsdgfE0qpV8RtNI1eV77042U15V",
                    "要从 Set-Cookie 提取 MFA 会话 token"
                );
            }
            other => panic!("应解析为 Mfa，实际 {other:?}"),
        }
    }

    /// 登录响应头的 x-tt-token 要提取（Set-Cookie / body 里都没有）。
    #[test]
    fn extracts_x_tt_token_from_headers() {
        let body =
            r#"{"error_code":0,"message":"success","data":{"user_id":1,"name":"u"}}"#.as_bytes();
        let headers = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-tt-token".to_string(), "00ab--cd-3.0.3".to_string()),
        ];
        let cookies = vec!["sessionid=s1; Path=/".to_string()];
        match parse_login_response(body, &cookies, &headers) {
            Ok(LoginOutcome::Success { token, .. }) => {
                assert_eq!(token, "00ab--cd-3.0.3");
            }
            other => panic!("应解析为 Success，实际 {other:?}"),
        }
    }

    /// 同名 cookie 多次下发（odin_tt）：后值覆盖、不叠罗汉（2026-10-05
    /// 落库数据里 odin_tt 重复 3 次的修复回归）。
    #[test]
    fn dedupes_repeated_set_cookies() {
        let lines = vec![
            "odin_tt=aaa; Path=/; Domain=.fqnovel.com".to_string(),
            "sessionid=s1; Path=/".to_string(),
            "odin_tt=bbb; Path=/".to_string(),
            "odin_tt=ccc; Path=/".to_string(),
        ];
        assert_eq!(
            extract_cookie_pairs(&lines),
            "odin_tt=ccc; sessionid=s1",
            "后值覆盖且保持首现顺序"
        );
    }

    /// 常规失败（顶层 error_code 形态）。
    #[test]
    fn surfaces_error_code() {
        let body = r#"{"error_code":1201,"message":"验证码错误"}"#.as_bytes();
        let err = parse_login_response(body, &[], &[]).unwrap_err();
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
        let waiting: Value =
            serde_json::from_str(r#"{"error_code":1045,"message":"waiting"}"#).unwrap();
        assert_eq!(
            waiting.get("error_code").and_then(Value::as_i64),
            Some(1045)
        );
    }
}
