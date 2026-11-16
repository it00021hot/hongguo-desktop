//! 一次注册尝试：身份材料在重试间保持不变；拒绝与限流不换身份。

use std::path::Path;
use std::time::Duration;

use chrono::DateTime;
use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::DeviceProfile;

use super::{
    REGISTER_ORIGIN, REGISTER_PATH, REGISTER_TIMEOUT_SECS, RegisterResult, parse_register,
    register_json_str, register_query, tt_encrypt_v5,
};
use crate::client::{ApiEnv, build_client};

const REUSE_ENV: &str = "HONGGUO_REUSE_DEVICE_JSON";
const REUSE_FIELDS: [&str; 4] = ["cdid", "openudid", "device_id", "iid"];

/// 显式只复用档案的路径。未设置表示允许注册。
pub fn reuse_json_path() -> Option<std::path::PathBuf> {
    std::env::var_os(REUSE_ENV).map(std::path::PathBuf::from)
}

/// 读并校验只复用档案。缺文件、读失败、JSON 损坏、身份字段空缺都在这里失败，不建连接。
pub fn load_reuse_profile(path: &Path) -> AppResult<DeviceProfile> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        AppError::InvalidArgs(format!("只复用档案无法读取 {}: {e}", path.display()))
    })?;
    let profile: DeviceProfile = serde_json::from_str(&raw)
        .map_err(|e| AppError::StoreCorrupt(format!("只复用档案损坏: {e}")))?;
    for key in REUSE_FIELDS {
        if profile.get(key).is_empty() {
            return Err(AppError::InvalidArgs(format!("只复用档案缺少 {key}")));
        }
    }
    if profile.ttreq().unwrap_or("").is_empty() {
        return Err(AppError::InvalidArgs("只复用档案缺少 ttreq".into()));
    }
    Ok(profile)
}

/// 同一次注册的身份材料。瞬态重试必须复用这一份，不能再抽新 cdid。
#[derive(Debug, Clone)]
pub struct RegisterMaterial {
    pub(super) cdid: String,
    pub(super) openudid: String,
    pub(super) clientudid: String,
    pub(super) req_id: String,
}

impl RegisterMaterial {
    pub fn mint() -> Self {
        Self {
            cdid: uuid_v4(),
            openudid: {
                let b: [u8; 8] = rand::random();
                b.iter().map(|x| format!("{x:02x}")).collect()
            },
            clientudid: uuid_v4(),
            req_id: uuid_v4(),
        }
    }

    pub fn cdid(&self) -> &str {
        &self.cdid
    }
}

fn uuid_v4() -> String {
    let b: [u8; 16] = rand::random();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6] & 0x0f | 0x40,
        b[7],
        b[8] & 0x3f | 0x80,
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

/// 服务端限制窗口。`Manual` 表示响应没有可解析的 Retry-After，禁止自动恢复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterBlock {
    Until { until_ms: i64 },
    Manual,
}

/// 一次注册尝试的失败。瞬态才允许用同一材料再试。
#[derive(Debug)]
pub enum RegisterAttemptError {
    Transient(String),
    Blocked(RegisterBlock),
    ReuseOnly,
    Failed(AppError),
}

impl From<RegisterAttemptError> for AppError {
    fn from(value: RegisterAttemptError) -> Self {
        match value {
            RegisterAttemptError::Transient(msg) => AppError::Network(msg),
            RegisterAttemptError::Blocked(RegisterBlock::Until { until_ms }) => {
                AppError::Rejected(format!("注册被拒绝，限制到 {until_ms}"))
            }
            RegisterAttemptError::Blocked(RegisterBlock::Manual) => {
                AppError::Rejected("注册被拒绝，未给出 Retry-After，不自动重试".into())
            }
            RegisterAttemptError::ReuseOnly => AppError::Rejected("只复用模式禁止注册".into()),
            RegisterAttemptError::Failed(err) => err,
        }
    }
}

/// `Retry-After` 秒数或 HTTP 日期。空、无法解析都不编造窗口，改为人工解除。
pub fn block_from_retry_after(header: Option<&str>, now_ms: i64) -> RegisterBlock {
    let Some(raw) = header.map(str::trim).filter(|s| !s.is_empty()) else {
        return RegisterBlock::Manual;
    };
    if let Ok(secs) = raw.parse::<u64>() {
        return RegisterBlock::Until {
            until_ms: now_ms.saturating_add((secs as i64).saturating_mul(1000)),
        };
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(raw) {
        return RegisterBlock::Until {
            until_ms: dt.timestamp_millis(),
        };
    }
    RegisterBlock::Manual
}

pub async fn register_material(
    env: &ApiEnv,
    material: &RegisterMaterial,
    origin: &str,
    now_ms: u64,
) -> Result<RegisterResult, RegisterAttemptError> {
    if reuse_json_path().is_some() {
        return Err(RegisterAttemptError::ReuseOnly);
    }
    if !origin_is_allowed(origin) {
        return Err(RegisterAttemptError::Failed(AppError::InvalidArgs(
            "注册端点只允许官方源或 loopback".into(),
        )));
    }
    post_register(env, material, origin, now_ms).await
}

fn origin_is_allowed(origin: &str) -> bool {
    if origin == REGISTER_ORIGIN {
        return true;
    }
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
}

async fn post_register(
    env: &ApiEnv,
    id: &RegisterMaterial,
    origin: &str,
    now_ms: u64,
) -> Result<RegisterResult, RegisterAttemptError> {
    let fp = hongguo_signer::device::fingerprint_for_cdid(&id.cdid);
    let body = tt_encrypt_v5(register_json_str(fp, id, now_ms).as_bytes());
    if std::env::var("TT_DEBUG").is_ok() {
        dump_debug(id, fp, now_ms, &body);
    }
    let query = register_query(&id.req_id);
    let url = format!("{origin}{REGISTER_PATH}?{query}");
    let response = build_client(&env.proxy)
        .map_err(RegisterAttemptError::Failed)?
        .post(url)
        .timeout(Duration::from_secs(REGISTER_TIMEOUT_SECS))
        .header("User-Agent", fp.register_ua())
        .header("Content-Type", "application/octet-stream;tt-data=a")
        .header("log-encode-type", "gzip")
        .header("x-ss-req-ticket", now_ms.to_string())
        .header("x-vc-bdturing-sdk-version", "4.0.3.cn")
        .header("x-neptune", "-8|50:51:59:30:40:47:49:39")
        .header("x-ss-stub", hongguo_signer::md5_hex_upper(&body))
        .body(body)
        .send()
        .await
        .map_err(|e| RegisterAttemptError::Transient(e.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        return Err(classify_status(status, retry_after.as_deref(), now_ms));
    }
    let set_cookies: Vec<String> = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| RegisterAttemptError::Transient(e.to_string()))?;
    let mut result = parse_register(&bytes).map_err(RegisterAttemptError::Failed)?;
    result.cdid = id.cdid.clone();
    result.openudid = id.openudid.clone();
    result.ttreq = set_cookies
        .iter()
        .find_map(|c| {
            let head = c.split(';').next()?.trim();
            head.strip_prefix("ttreq=").map(str::to_string)
        })
        .unwrap_or_default();
    Ok(result)
}

fn classify_status(status: u16, retry_after: Option<&str>, now_ms: u64) -> RegisterAttemptError {
    if status == 408 || (500..600).contains(&status) {
        return RegisterAttemptError::Transient(format!("HTTP {status}"));
    }
    RegisterAttemptError::Blocked(block_from_retry_after(retry_after, now_ms as i64))
}

fn dump_debug(
    id: &RegisterMaterial,
    fp: &hongguo_signer::device::RegisterFingerprint,
    now_ms: u64,
    body: &[u8],
) {
    let dir = crate::capture_dir();
    let _ = std::fs::create_dir_all(&dir);
    let plain = register_json_str(fp, id, now_ms);
    let _ = hongguo_common::file_io::write(&dir.join("rust_reg_plain.json"), plain.as_bytes());
    let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
    let _ = hongguo_common::file_io::write(&dir.join("rust_reg_body.hex"), hex.as_bytes());
    let meta = serde_json::json!({
        "cdid": id.cdid,
        "openudid": id.openudid,
        "clientudid": id.clientudid,
        "req_id": id.req_id
    });
    let _ = hongguo_common::file_io::write(
        &dir.join("rust_reg_meta.json"),
        meta.to_string().as_bytes(),
    );
}
