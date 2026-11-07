//! 一次注册尝试：身份材料在重试间保持不变；拒绝与限流不换身份。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
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

static HTTP_SENDS: AtomicU32 = AtomicU32::new(0);

/// 已发往注册端点的 HTTP 次数（含连接失败）。只复用模式在发请求前返回，不计数。
pub fn register_http_sends() -> u32 {
    HTTP_SENDS.load(Ordering::Relaxed)
}

pub fn reset_register_http_sends() {
    HTTP_SENDS.store(0, Ordering::Relaxed);
}

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

    #[cfg(test)]
    pub fn from_parts(cdid: &str, openudid: &str, clientudid: &str, req_id: &str) -> Self {
        Self {
            cdid: cdid.to_string(),
            openudid: openudid.to_string(),
            clientudid: clientudid.to_string(),
            req_id: req_id.to_string(),
        }
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

pub async fn register_device(env: &ApiEnv) -> AppResult<RegisterResult> {
    let now_ms = now_millis();
    register_material(env, &RegisterMaterial::mint(), REGISTER_ORIGIN, now_ms)
        .await
        .map_err(AppError::from)
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

pub async fn register_retrying(
    env: &ApiEnv,
    material: &RegisterMaterial,
    origin: &str,
    max_attempts: usize,
    pause: Duration,
) -> Result<RegisterResult, RegisterAttemptError> {
    if reuse_json_path().is_some() {
        return Err(RegisterAttemptError::ReuseOnly);
    }
    let mut last = RegisterAttemptError::Transient("没有注册尝试".into());
    for attempt in 0..max_attempts {
        let now_ms = now_millis();
        match register_material(env, material, origin, now_ms).await {
            Ok(result) => return Ok(result),
            Err(err @ RegisterAttemptError::Transient(_)) => {
                last = err;
                if attempt + 1 == max_attempts {
                    break;
                }
                tokio::time::sleep(pause).await;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    let query = register_query()
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    HTTP_SENDS.fetch_add(1, Ordering::Relaxed);
    let response = build_client(&env.proxy)
        .map_err(RegisterAttemptError::Failed)?
        .post(format!("{origin}{REGISTER_PATH}?{query}"))
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
    if result.device_id == "0" {
        return Err(RegisterAttemptError::Failed(AppError::Media(
            "注册被服务端静默拒绝（device_id=0）".into(),
        )));
    }
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
    let _ = std::fs::write(dir.join("rust_reg_plain.json"), &plain);
    let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::write(dir.join("rust_reg_body.hex"), &hex);
    let meta = serde_json::json!({
        "cdid": id.cdid,
        "openudid": id.openudid,
        "clientudid": id.clientudid,
        "req_id": id.req_id
    });
    let _ = std::fs::write(dir.join("rust_reg_meta.json"), meta.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    static LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
        LOCK.lock().await
    }

    fn env() -> ApiEnv {
        ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default())
    }

    #[test]
    fn retry_after_seconds_and_http_date() {
        assert_eq!(
            block_from_retry_after(Some("60"), 1_000),
            RegisterBlock::Until { until_ms: 61_000 }
        );
        assert_eq!(
            block_from_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT"), 0),
            RegisterBlock::Until {
                until_ms: 1_445_412_480_000
            }
        );
        assert_eq!(block_from_retry_after(None, 0), RegisterBlock::Manual);
        assert_eq!(
            block_from_retry_after(Some("soon"), 0),
            RegisterBlock::Manual
        );
    }

    #[test]
    fn reuse_profile_fails_closed_before_any_shape_guess() {
        let _guard = LOCK.blocking_lock();
        let missing = std::env::temp_dir().join(format!("hg-reuse-missing-{}", std::process::id()));
        let err = load_reuse_profile(&missing).expect_err("缺文件");
        assert!(err.to_string().contains("无法读取"));

        let broken = std::env::temp_dir().join(format!("hg-reuse-broken-{}", std::process::id()));
        std::fs::write(&broken, "{").unwrap();
        assert!(
            load_reuse_profile(&broken)
                .unwrap_err()
                .to_string()
                .contains("损坏")
        );

        let partial = std::env::temp_dir().join(format!("hg-reuse-partial-{}", std::process::id()));
        std::fs::write(&partial, r#"{"fields":[],"ttreq":"t"}"#).unwrap();
        assert!(
            load_reuse_profile(&partial)
                .unwrap_err()
                .to_string()
                .contains("缺少")
        );
        let _ = std::fs::remove_file(&broken);
        let _ = std::fs::remove_file(&partial);
    }

    #[tokio::test]
    async fn reuse_mode_sends_zero_register_requests() {
        let _guard = lock().await;
        reset_register_http_sends();
        let path = std::env::temp_dir().join(format!("hg-reuse-set-{}", std::process::id()));
        std::fs::write(&path, "{}").unwrap();
        let prev = std::env::var_os(REUSE_ENV);
        unsafe { std::env::set_var(REUSE_ENV, &path) };
        let material = RegisterMaterial::mint();
        let err = register_material(&env(), &material, REGISTER_ORIGIN, 1)
            .await
            .expect_err("只复用");
        assert!(matches!(err, RegisterAttemptError::ReuseOnly));
        assert_eq!(register_http_sends(), 0);
        unsafe {
            match prev {
                Some(v) => std::env::set_var(REUSE_ENV, v),
                None => std::env::remove_var(REUSE_ENV),
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejection_stops_and_transient_keeps_material() {
        let _guard = lock().await;
        reset_register_http_sends();
        let (origin, bodies) =
            loopback("429 Too Many Requests", "Retry-After: 60\r\n", "", 1).await;
        let material = RegisterMaterial::mint();
        let err = register_retrying(&env(), &material, &origin, 3, Duration::from_millis(0))
            .await
            .expect_err("429");
        match err {
            RegisterAttemptError::Blocked(RegisterBlock::Until { until_ms }) => {
                assert!(until_ms > 0);
            }
            other => panic!("应为限制窗口，实际 {other:?}"),
        }
        assert_eq!(register_http_sends(), 1, "拒绝后不得换身份再试");
        assert_eq!(bodies.lock().unwrap().len(), 1);

        reset_register_http_sends();
        let (origin, bodies) = loopback("503 Service Unavailable", "", "", 2).await;
        let material = RegisterMaterial::mint();
        let _ = register_retrying(&env(), &material, &origin, 2, Duration::from_millis(0))
            .await
            .expect_err("503");
        assert_eq!(register_http_sends(), 2);
        let saved = bodies.lock().unwrap().clone();
        assert_eq!(saved.len(), 2);
        let first = cdid_of(&saved[0]);
        let second = cdid_of(&saved[1]);
        assert_eq!(first, second);
        assert_eq!(first, material.cdid());
    }

    #[tokio::test]
    async fn rejection_without_retry_after_is_manual() {
        let _guard = lock().await;
        reset_register_http_sends();
        let (origin, _) = loopback("403 Forbidden", "", "", 1).await;
        let err = register_material(&env(), &RegisterMaterial::mint(), &origin, 5)
            .await
            .expect_err("403");
        assert!(matches!(
            err,
            RegisterAttemptError::Blocked(RegisterBlock::Manual)
        ));
        assert_eq!(register_http_sends(), 1);
    }

    #[tokio::test]
    async fn non_loopback_custom_origin_is_rejected_without_send() {
        let _guard = lock().await;
        reset_register_http_sends();
        let err =
            register_material(&env(), &RegisterMaterial::mint(), "https://example.com", 1).await;
        assert!(matches!(err, Err(RegisterAttemptError::Failed(_))));
        assert_eq!(register_http_sends(), 0);
    }

    fn cdid_of(body: &[u8]) -> String {
        let (plain, _) = super::super::tt_decrypt_v5(body).expect("夹具请求体应能解密");
        let v: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        v["header"]["cdid"].as_str().unwrap().to_string()
    }

    async fn loopback(
        status: &'static str,
        extra_headers: &'static str,
        body: &'static str,
        accepts: usize,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let saved = std::sync::Arc::clone(&seen);
        tokio::spawn(async move {
            for _ in 0..accepts {
                let Ok((mut sock, peer)) = listener.accept().await else {
                    break;
                };
                if peer.ip().to_string() != "127.0.0.1" {
                    break;
                }
                let body_bytes = read_body(&mut sock).await;
                saved.lock().unwrap().push(body_bytes);
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
                    body.len()
                );
                use tokio::io::AsyncWriteExt;
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (format!("http://{addr}"), seen)
    }

    async fn read_body(sock: &mut tokio::net::TcpStream) -> Vec<u8> {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = sock.read(&mut tmp).await.unwrap_or(0);
            if n == 0 {
                return buf;
            }
            buf.extend_from_slice(&tmp[..n]);
            let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let header = String::from_utf8_lossy(&buf[..header_end]);
            let len = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            let start = header_end + 4;
            while buf.len() < start + len {
                let n = sock.read(&mut tmp).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            return buf.get(start..start + len).unwrap_or_default().to_vec();
        }
    }
}
