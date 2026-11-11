//! 接口调用：重试与空响应判定。
//!
//! ⚠️ 服务端签名失效返回 HTTP 200 + 0 字节——必须检查响应体长度，
//! 只看状态码会把失败当成功。

use std::time::Duration;

use reqwest::Proxy;

use hongguo_common::{AppError, AppResult};

const MAX_RETRIES: u32 = 3;

pub const LQ_API_ORIGIN: &str = "https://api5-normal-lq.fqnovel.com";

pub(crate) const SINFONLINEA_API_ORIGIN: &str = "https://api5-normal-sinfonlinea.fqnovel.com";

pub const SINFONLINEC_API_ORIGIN: &str = "https://api5-normal-sinfonlinec.fqnovel.com";

pub const LF_API_ORIGIN: &str = "https://api5-normal-lf.fqnovel.com";

/// 新通道取流 origin；失败回退 [`LQ_API_ORIGIN`]。
pub const READING_API_ORIGIN: &str = "https://reading.snssdk.com";

const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);

/// 与 service 的在线填充侧同源，改这里一处。
pub const STREAM_RETRY_ATTEMPTS: u32 = 3;

pub fn stream_retry_backoff(attempt: u32) -> Duration {
    Duration::from_millis(300 * (attempt as u64 + 1))
}

/// 按配置取 HTTP client（进程级按解析后的代理地址缓存；改代理即换池，免重启）。
pub fn build_client(config: &hongguo_common::model::ProxyConfig) -> AppResult<reqwest::Client> {
    static CLIENTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, reqwest::Client>>,
    > = std::sync::OnceLock::new();
    let key = if hongguo_common::offline_qa::enabled() {
        "offline-qa".to_string()
    } else {
        config.resolved().unwrap_or_default()
    };
    let clients = CLIENTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(client) = guard.get(&key) {
        return Ok(client.clone());
    }
    let client = build_client_uncached(config)?;
    guard.insert(key, client.clone());
    Ok(client)
}

/// UA 与超时是全局契约，不随代理配置变。
fn build_client_uncached(
    config: &hongguo_common::model::ProxyConfig,
) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(hongguo_signer::video_ua())
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));
    // Android：platform-verifier 需要 JNI 初始化，缺失时 TLS 全挂，喂 webpki 根
    #[cfg(target_os = "android")]
    let mut builder =
        builder.use_preconfigured_tls(hongguo_common::utils::tls::webpki_tls_config()?);

    if hongguo_common::offline_qa::enabled() {
        // 配置里的代理和默认重定向都能把 loopback 请求带到外部主机。开关打开时这两条都不走。
        builder = builder.no_proxy().redirect(offline_redirect_policy());
    } else {
        match config.resolved() {
            Some(url) => {
                let proxy = Proxy::all(&url)
                    .map_err(|e| AppError::Network(format!("代理地址无效: {e}")))?;
                builder = builder.proxy(proxy);
            }
            None => {
                builder = builder.no_proxy();
            }
        }
    }

    builder
        .build()
        .map_err(|e| AppError::Network(format!("构造 HTTP client 失败: {e}")))
}

/// 一次官方 API 调用的全部环境（从 `AppState` 快照，以值传递防半新半旧）。
#[derive(Debug, Clone)]
pub struct ApiEnv {
    pub proxy: hongguo_common::model::ProxyConfig,
    pub device: hongguo_signer::device::DeviceProfile,
    /// 会话 Cookie（不参与签名，签名后注入）。
    pub cookie: Option<String>,
    /// x-tt-token 长凭据；请求带前 56 位短形式。
    pub x_tt_token: Option<String>,
}

impl ApiEnv {
    /// 未登录默认环境（匿名游客档案）。不 `#[cfg(test)]` 门控：下游
    /// crate 的测试要用。
    pub fn anonymous(proxy: hongguo_common::model::ProxyConfig) -> Self {
        Self {
            proxy,
            device: hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid()),
            cookie: None,
            x_tt_token: None,
        }
    }
}

/// 调用官方 App 接口，返回响应体字节。
pub async fn api_call(pathname: &str, body: Option<Vec<u8>>, env: &ApiEnv) -> AppResult<Vec<u8>> {
    api_call_full(hongguo_signer::API_ORIGIN, pathname, body, &[], env).await
}

/// 全参数形态（origin + 业务 query + body）。
pub async fn api_call_full(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    api_call_full_with_headers(origin, pathname, body, biz_query, &[], env).await
}

/// reading 形态（gzip body）；空响应不重试不报错。
pub async fn api_call_reading_once_allow_empty(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    let mut headers = reading_headers();
    let body = match body {
        Some(raw) if !raw.is_empty() => {
            headers.push(("Content-Encoding".into(), "gzip".into()));
            Some(gzip_bytes(&raw)?)
        }
        other => other,
    };
    let client = build_client(&env.proxy)?;
    let resp = send_once(
        &client,
        origin,
        pathname,
        body.as_deref(),
        biz_query,
        &headers,
        env,
    )
    .await?;
    Ok(resp.bytes)
}

const STORE_REGION: &str = "cn-gd";

/// 取流请求级快照（HG_DUMP_REQ_DIR=目录时写入）。用于同进程 PLAY_OK vs 二次进程空的报文对照。
/// 会话票据只保留字段名与长度。
fn dump_signed_play_req(
    tag: &str,
    origin: &str,
    pathname: &str,
    signed: &hongguo_signer::ticket::SignedRequest,
    env: &ApiEnv,
    resp_len: usize,
) {
    let Some(dir) = std::env::var_os("HG_DUMP_REQ_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&dir);
    let seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let label = std::env::var("HG_DUMP_REQ_TAG").unwrap_or_else(|_| tag.to_string());

    let url = &signed.url;
    let (path_part, query_part) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url.clone(), String::new()),
    };
    // path 可能含 origin
    let path_only = path_part
        .strip_prefix(origin)
        .unwrap_or(pathname)
        .to_string();

    let mut query_map = serde_json::Map::new();
    for pair in query_part.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let value = if k == "klink_egdi" {
            serde_json::json!({ "length": v.len() })
        } else {
            serde_json::Value::String(v.to_string())
        };
        query_map.insert(k.to_string(), value);
    }

    let mut headers = serde_json::Map::new();
    let mut cookie_keys: Vec<String> = Vec::new();
    let mut cookie_has_ttreq = false;
    let mut cookie_ttreq_len = None;
    let mut cookie_has_install_id = false;
    for (k, v) in &signed.headers {
        let lk = k.to_ascii_lowercase();
        if lk == "cookie" {
            for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
                if let Some((ck, cv)) = part.split_once('=') {
                    cookie_keys.push(ck.to_string());
                    if ck == "ttreq" {
                        cookie_has_ttreq = true;
                        cookie_ttreq_len = Some(cv.len());
                    }
                    if ck == "install_id" || ck == "iid" {
                        cookie_has_install_id = true;
                    }
                }
            }
            headers.insert(
                k.clone(),
                serde_json::json!({
                    "keys": cookie_keys,
                    "has_ttreq": cookie_has_ttreq,
                    "ttreq_len": cookie_ttreq_len,
                    "has_install_id": cookie_has_install_id,
                    "raw_len": v.len(),
                }),
            );
        } else if lk == "x-tt-token" {
            headers.insert(k.clone(), serde_json::json!({ "length": v.len() }));
        } else {
            headers.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
    }

    let body_len = signed.body.as_ref().map(|b| b.len()).unwrap_or(0);
    let body_sha = signed
        .body
        .as_ref()
        .map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>())
        .unwrap_or_default();
    let body_raw_preview = signed.body.as_ref().and_then(|gz| {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut d = GzDecoder::new(gz.as_slice());
        let mut raw = Vec::new();
        d.read_to_end(&mut raw).ok()?;
        Some(String::from_utf8_lossy(&raw).into_owned())
    });

    let iid = env.device.get("iid").to_string();
    let cdid = env.device.get("cdid").to_string();
    let device_id = env.device.get("device_id").to_string();
    let device_ttreq_len = env.device.ttreq().map(str::len);

    let snap = serde_json::json!({
        "tag": label,
        "origin": origin,
        "pathname": pathname,
        "path_in_url": path_only,
        "url_len": url.len(),
        "query": query_map,
        "headers": headers,
        "body_gzip_hex": body_sha,
        "body_gzip_len": body_len,
        "body_ungzip_utf8": body_raw_preview,
        "device": {
            "iid": iid,
            "cdid": cdid,
            "device_id": device_id,
            "ttreq_len": device_ttreq_len,
            "brand": env.device.get("device_brand"),
            "type": env.device.get("device_type"),
        },
        "env": {
            "has_cookie": env.cookie.as_ref().map(|c| !c.is_empty()).unwrap_or(false),
            "has_x_tt_token": env.x_tt_token.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
            "x_tt_token_len": env.x_tt_token.as_ref().map(|t| t.len()).unwrap_or(0),
        },
        "resp_bytes": resp_len,
    });

    let file = dir.join(format!("{seq}-{label}.json"));
    if let Ok(s) = serde_json::to_string_pretty(&snap) {
        let _ = std::fs::write(&file, s);
        // also append one-line index
        let idx = dir.join("INDEX.txt");
        let line = format!(
            "{seq}\t{label}\tiid={}\tresp_bytes={resp_len}\tfile={}\n",
            env.device.get("iid"),
            file.file_name().and_then(|n| n.to_str()).unwrap_or("?")
        );
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(idx)
            .ok();
        if let Some(ref mut f) = f {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
        log::info!("[DumpReq] wrote {}", file.display());
    }
}

/// 播放请求使用当前身份的 MetaSec v5 会话签名。
pub(crate) async fn api_call_playback_v5(
    origin: &str,
    pathname: &str,
    payload: Vec<u8>,
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    let mut extra = vec![
        ("x-ss-dp".to_string(), READING_DP.into()),
        ("lc".to_string(), LC.into()),
        ("x-tt-store-region".to_string(), STORE_REGION.into()),
        ("x-tt-store-region-src".to_string(), "did".into()),
        ("Content-Encoding".to_string(), "gzip".into()),
    ];
    if let Some(c) = &env.cookie
        && !c.is_empty()
    {
        extra.push(("Cookie".to_string(), c.clone()));
    }
    // 登录态短形式 token：缺了被服务端按匿名配额处理
    if let Some(full) = &env.x_tt_token
        && !full.is_empty()
    {
        let short: String = full.chars().take(56).collect();
        extra.push(("x-tt-token".into(), short));
    }

    refuse_external(origin)?;
    let client = build_client(&env.proxy)?;
    let body = gzip_bytes(&payload)?;
    let signed =
        crate::metasec::sign_playback(&client, origin, pathname, &body, &extra, env).await?;
    refuse_external(&signed.url)?;
    let mut req = client.post(&signed.url);
    for (k, v) in &signed.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    // clone body for dump after move into req
    let body_for_dump = signed.body.clone();
    let signed_for_dump = hongguo_signer::ticket::SignedRequest {
        url: signed.url.clone(),
        headers: signed.headers.clone(),
        body: body_for_dump,
    };
    let resp = req
        .body(signed.body.unwrap_or_default())
        .send()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Network(format!("HTTP {status}")));
    }
    let bytes = resp
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;
    log::debug!("[PlayUrl] {pathname} HTTP {status}, {} bytes", bytes.len());
    dump_signed_play_req(
        "playback_v5",
        origin,
        pathname,
        &signed_for_dump,
        env,
        bytes.len(),
    );
    Ok(bytes)
}

/// [`api_call_full`] 的带额外头版本。
pub async fn api_call_full_with_headers(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    api_call_full_response(origin, pathname, body, biz_query, extra_headers, env)
        .await
        .map(|r| r.bytes)
}

/// 响应体 + Set-Cookie + 全部响应头（登录捕获会话 / x-tt-token 用）。
pub struct ApiCallResponse {
    pub bytes: Vec<u8>,
    /// Set-Cookie 原始行（未清洗）。
    pub set_cookies: Vec<String>,
    /// 全部响应头（小写名）。
    pub headers: Vec<(String, String)>,
}

/// reading 系统一入口：轻签名头 + POST body 一律 gzip。
pub async fn api_call_reading(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    let mut headers = reading_headers();
    let body = match body {
        Some(raw) if !raw.is_empty() => {
            headers.push(("Content-Encoding".into(), "gzip".into()));
            Some(gzip_bytes(&raw)?)
        }
        other => other,
    };
    api_call_full_with_headers(origin, pathname, body, biz_query, &headers, env).await
}

/// body 原样（multipart 上传），签名仍按原始 body 算摘要。
pub async fn api_call_reading_raw(
    origin: &str,
    pathname: &str,
    body: Vec<u8>,
    content_type: &str,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    let mut headers = reading_headers();
    headers.push(("Content-Type".into(), content_type.to_string()));
    api_call_full_with_headers(origin, pathname, Some(body), biz_query, &headers, env).await
}

/// gzip 压缩（reading 系 POST body 形态）。
pub fn gzip_bytes(raw: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(raw)
        .and_then(|_| encoder.finish())
        .map_err(|e| AppError::Network(format!("gzip 压缩失败: {e}")))
}

/// x-ss-dp：红果 App 的 aid。
const READING_DP: &str = "8662";
/// lc：reading 系固定头。
const LC: &str = "101";

/// reading 系轻签名头（不带 x-gorgon 族，只要这三样）。
pub fn reading_headers() -> Vec<(String, String)> {
    let ticket_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    vec![
        ("x-ss-dp".into(), READING_DP.into()),
        ("lc".into(), LC.into()),
        (
            "x-reading-request".into(),
            format!("{ticket_ms}-{}", rand::random::<u32>()),
        ),
    ]
}

/// 全参数 + Set-Cookie 捕获版本。
pub async fn api_call_full_response(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<ApiCallResponse> {
    let client = build_client(&env.proxy)?;
    let mut last_err = String::new();

    for attempt in 0..MAX_RETRIES {
        match send_once(
            &client,
            origin,
            pathname,
            body.as_deref(),
            biz_query,
            extra_headers,
            env,
        )
        .await
        {
            Ok(resp) if !resp.bytes.is_empty() => return Ok(resp),
            Ok(_) => {
                log::warn!(
                    "[Api] 空响应：{origin}{pathname}（biz_query={} 项，body={} 字节）",
                    biz_query.len(),
                    body.as_deref().map(|b| b.len()).unwrap_or(0)
                );
                last_err = format!("{origin}{pathname}，连续 {MAX_RETRIES} 次空响应");
            }
            Err(e) => last_err = e.to_string(),
        }
        if attempt + 1 < MAX_RETRIES {
            tokio::time::sleep(RETRY_BASE_DELAY * (attempt + 1)).await;
        }
    }

    Err(AppError::EmptyResponse(last_err))
}

/// 请求视频 CDN 直链（下载与在线取流共用）：裸 UA 优先，403 补官网
/// Referer 重试一次。
pub async fn get_video_stream(client: &reqwest::Client, url: &str) -> AppResult<reqwest::Response> {
    refuse_external(url)?;
    let send = |referer: bool| {
        let mut req = client
            .get(url)
            .header("User-Agent", hongguo_signer::video_ua())
            // MP4 已压缩，HTTP gzip 会破坏 Range 偏移：必须按原始字节收
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if referer {
            req = req.header("Referer", hongguo_signer::VIDEO_REFERER);
        }
        req
    };

    let first = send(false)
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if first.status().as_u16() != 403 {
        return ensure_stream(url, first);
    }

    log::info!("[CDN] 直链被 403 拒（裸 UA），补 Referer 重试一次");
    let second = send(true)
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    ensure_stream(url, second)
}

/// 把非成功状态转成统一口径的错误。
fn ensure_stream(url: &str, resp: reqwest::Response) -> AppResult<reqwest::Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        Err(AppError::Network(format!(
            "取流失败: HTTP {}（{}）",
            resp.status(),
            host_of(url)
        )))
    }
}

fn host_of(url: &str) -> &str {
    url.split("//")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .unwrap_or(url)
}

/// 探测请求的总超时（只取 1 字节）。
const VIDEO_RANGE_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// 等响应头的上限。
const VIDEO_RANGE_HEADER_TIMEOUT: Duration = Duration::from_secs(15);

/// 读取停滞超时：「卡死」与「慢但仍在传」的分界。
const VIDEO_RANGE_STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// 单次 Range 请求总上限（兜底假活跃连接）。
const VIDEO_RANGE_TOTAL_TIMEOUT: Duration = Duration::from_secs(120);

/// `Range: bytes=0-0` 探测直链总长；CDN 不支持 Range（回 200）即 Err。
pub async fn probe_video_len(client: &reqwest::Client, url: &str) -> AppResult<u64> {
    refuse_external(url)?;
    let resp = client
        .get(url)
        .timeout(VIDEO_RANGE_PROBE_TIMEOUT)
        .header("User-Agent", hongguo_signer::video_ua())
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header("Range", "bytes=0-0")
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(AppError::Network(format!(
            "CDN 不支持 Range 请求: HTTP {}",
            resp.status()
        )));
    }
    let cr = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Network("206 响应缺少 Content-Range".into()))?;
    // 形如 `bytes 0-0/123456`；`*` 表示总量未知
    cr.rsplit('/')
        .next()
        .and_then(|t| t.parse::<u64>().ok())
        .filter(|t| *t > 0)
        .ok_or_else(|| AppError::Network(format!("Content-Range 无法解析总长: {cr}")))
}

/// 取直链的一段密文（失败重试 3 次；CDN 忽略 Range 回 200 时按字节
/// 切片自愈；403 补 Referer 重试一次）。
fn offline_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if hongguo_common::offline_qa::parsed_url_forbidden(attempt.url()) {
            attempt.error(std::io::Error::other("离线 QA 拒绝重定向到外部地址"))
        } else {
            attempt.follow()
        }
    })
}

pub fn refuse_external_send(url: &str) -> AppResult<()> {
    refuse_external(url)
}

fn refuse_external(url: &str) -> AppResult<()> {
    if hongguo_common::offline_qa::external_url_forbidden(url) {
        return Err(AppError::Network(
            "离线 QA 拒绝外部请求，且不改变注册阻断".into(),
        ));
    }
    Ok(())
}

pub async fn get_video_range(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    end: u64,
) -> AppResult<Vec<u8>> {
    refuse_external(url)?;
    let mut last_err = None;
    for attempt in 0..STREAM_RETRY_ATTEMPTS {
        let started = std::time::Instant::now();
        match get_video_range_once(client, url, start, end).await {
            Ok(bytes) => {
                crate::resolution_policy::record_transfer(bytes.len(), started.elapsed());
                return Ok(bytes);
            }
            Err(e) => {
                log::warn!("[CDN] Range {start}-{end} 第 {} 次失败: {e}", attempt + 1);
                last_err = Some(e);
                if attempt + 1 < STREAM_RETRY_ATTEMPTS {
                    tokio::time::sleep(stream_retry_backoff(attempt)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::Network("Range 请求重试耗尽".into())))
}

async fn get_video_range_once(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    end: u64,
) -> AppResult<Vec<u8>> {
    let send = |referer: bool| {
        let mut req = client
            .get(url)
            .timeout(VIDEO_RANGE_TOTAL_TIMEOUT)
            .header("User-Agent", hongguo_signer::video_ua())
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .header("Range", format!("bytes={start}-{end}"));
        if referer {
            req = req.header("Referer", hongguo_signer::VIDEO_REFERER);
        }
        req
    };
    let mut resp = send_range_request(send(false), start, end).await?;
    if resp.status().as_u16() == 403 {
        log::info!("[CDN] Range 请求被 403 拒（裸 UA），补 Referer 重试一次");
        resp = send_range_request(send(true), start, end).await?;
    }
    let status = resp.status();
    if status != reqwest::StatusCode::PARTIAL_CONTENT && status != reqwest::StatusCode::OK {
        return Err(AppError::Network(format!(
            "CDN 不支持 Range 请求: HTTP {status}"
        )));
    }

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let stalled = || {
        AppError::Network(format!(
            "Range {start}-{end} 读取停滞超过 {}s（无新字节）",
            VIDEO_RANGE_STALL_TIMEOUT.as_secs()
        ))
    };
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        // 206：响应体就是请求的那段
        let mut bytes = Vec::with_capacity((end - start + 1) as usize);
        loop {
            match tokio::time::timeout(VIDEO_RANGE_STALL_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(chunk))) => bytes.extend_from_slice(&chunk),
                Ok(Some(Err(e))) => return Err(AppError::Network(e.to_string())),
                Ok(None) => break,
                Err(_) => return Err(stalled()),
            }
        }
        Ok(bytes)
    } else {
        // 200：CDN 忽略了 Range 回全量——边收边切，收满即断流
        let mut cutter = RangeCutter::new(start, end);
        loop {
            match tokio::time::timeout(VIDEO_RANGE_STALL_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    if cutter.push(&chunk) {
                        break;
                    }
                }
                Ok(Some(Err(e))) => return Err(AppError::Network(e.to_string())),
                Ok(None) => break,
                Err(_) => return Err(stalled()),
            }
        }
        cutter.finish()
    }
}

/// 发一次 Range 请求，只对「等响应头」计时；body 的停滞由调用方逐块计时。
async fn send_range_request(
    request: reqwest::RequestBuilder,
    start: u64,
    end: u64,
) -> AppResult<reqwest::Response> {
    match tokio::time::timeout(VIDEO_RANGE_HEADER_TIMEOUT, request.send()).await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(AppError::Network(e.to_string())),
        Err(_) => Err(AppError::Network(format!(
            "Range {start}-{end} 等响应头超时（{}s）",
            VIDEO_RANGE_HEADER_TIMEOUT.as_secs()
        ))),
    }
}

/// 200 全量响应里切 `[start, end]` 的状态机（独立成结构以便单测）。
struct RangeCutter {
    skip_left: u64,
    want: usize,
    out: Vec<u8>,
}

impl RangeCutter {
    fn new(start: u64, end: u64) -> Self {
        Self {
            skip_left: start,
            want: (end - start + 1) as usize,
            out: Vec::new(),
        }
    }

    /// 喂一段响应字节；true = 目标区间已收满，调用方应立即断流。
    fn push(&mut self, chunk: &[u8]) -> bool {
        let mut rest = chunk;
        if self.skip_left > 0 {
            let skip = (self.skip_left as usize).min(rest.len());
            rest = &rest[skip..];
            self.skip_left -= skip as u64;
        }
        if self.out.len() < self.want {
            let take = (self.want - self.out.len()).min(rest.len());
            self.out.extend_from_slice(&rest[..take]);
        }
        self.out.len() >= self.want
    }

    /// 取出切好的区间；一个字节都没落到说明 start 越界，按错误处理。
    fn finish(self) -> AppResult<Vec<u8>> {
        if self.out.is_empty() {
            return Err(AppError::Network("200 全量响应未覆盖请求区间的起点".into()));
        }
        Ok(self.out)
    }
}

/// 签名在此生成；Cookie 不参与签名，签名后注入。
async fn send_once(
    client: &reqwest::Client,
    origin: &str,
    pathname: &str,
    body: Option<&[u8]>,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<ApiCallResponse> {
    if hongguo_common::offline_qa::external_url_forbidden(origin) {
        return Err(AppError::Network(
            "离线 QA 拒绝外部请求，且不改变注册阻断".into(),
        ));
    }
    let mut extra: Vec<(String, String)> = extra_headers.to_vec();
    if let Some(c) = &env.cookie
        && !c.is_empty()
    {
        extra.push(("Cookie".into(), c.clone()));
    }
    if let Some(full) = &env.x_tt_token
        && !full.is_empty()
    {
        let short: String = full.chars().take(56).collect();
        extra.push(("x-tt-token".into(), short));
    }
    let signed = match body {
        Some(bytes) => hongguo_signer::sign_request_with(
            origin,
            pathname,
            Some(bytes.to_vec()),
            &env.device,
            biz_query,
            &extra,
        ),
        None => hongguo_signer::sign_request_with(
            origin,
            pathname,
            None,
            &env.device,
            biz_query,
            &extra,
        ),
    };

    if hongguo_common::offline_qa::external_url_forbidden(&signed.url) {
        return Err(AppError::Network(
            "离线 QA 拒绝外部请求，且不改变注册阻断".into(),
        ));
    }
    let mut req = match &signed.body {
        Some(bytes) => client.post(&signed.url).body(bytes.clone()),
        None => client.get(&signed.url),
    };
    for (k, v) in &signed.headers {
        req = req.header(k, v);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!("HTTP {}", resp.status())));
    }
    let set_cookies = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))
        })
        .collect();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;
    Ok(ApiCallResponse {
        bytes: bytes.to_vec(),
        set_cookies,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_common::model::ProxyConfig;
    use hongguo_common::model::settings::ProxyMode;
    use hongguo_signer::ticket::{sign_get, sign_post};

    #[test]
    fn range_cutter_slices_target_from_full_body() {
        let body: Vec<u8> = (0..100u8).collect();
        let mut cutter = RangeCutter::new(30, 49);
        assert!(!cutter.push(&body[0..40]), "还差 10 字节");
        assert!(cutter.push(&body[40..80]), "收满 20 字节即止");
        assert_eq!(cutter.finish().unwrap(), &body[30..50]);
    }

    #[test]
    fn range_cutter_from_zero_takes_head() {
        let body: Vec<u8> = (0..255u8).collect();
        let mut cutter = RangeCutter::new(0, 9);
        assert!(cutter.push(&body));
        assert_eq!(cutter.finish().unwrap(), &body[0..10]);
    }

    #[test]
    fn range_cutter_short_body_gives_what_exists() {
        let body: Vec<u8> = vec![9u8; 5];
        let mut cutter = RangeCutter::new(0, 99);
        cutter.push(&body);
        assert_eq!(cutter.finish().unwrap(), body);
    }

    #[test]
    fn range_cutter_start_beyond_body_is_error() {
        let mut cutter = RangeCutter::new(50, 60);
        cutter.push(&[1u8; 10]);
        assert!(cutter.finish().is_err(), "起点都没落到，必须报错");
    }

    #[test]
    fn retry_count_is_sane() {
        assert!((1..=5).contains(&MAX_RETRIES));
    }

    #[test]
    fn post_signature_always_has_body() {
        let signed = sign_post(
            "/x/v1/",
            b"{}".to_vec(),
            &ApiEnv::anonymous(ProxyConfig::default()).device,
        );
        assert!(signed.body.is_some(), "POST 签名必须带 body");
    }

    #[test]
    fn get_signature_has_no_body() {
        let signed = sign_get("/x/v1/", &ApiEnv::anonymous(ProxyConfig::default()).device);
        assert!(signed.body.is_none(), "GET 签名不应带 body");
    }

    #[tokio::test]
    async fn offline_userinfo_host_returns_before_send() {
        let previous = std::env::var_os(hongguo_common::offline_qa::ENV);
        unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, "1") };
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let err = get_video_stream(&client, "http://localhost:80@example.com/a")
            .await
            .expect_err("userinfo 里的 localhost 不是主机");
        assert!(err.to_string().contains("离线 QA"));
        let err = get_video_range(&client, "http://127.0.0.1:80@example.com/a", 0, 1)
            .await
            .expect_err("userinfo 里的 127.0.0.1 不是主机");
        assert!(err.to_string().contains("离线 QA"));
        match previous {
            Some(value) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, value) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
    }

    #[test]
    fn client_builds_for_default_proxy() {
        let _ = build_client(&ProxyConfig::default());
    }

    #[test]
    fn direct_mode_builds_no_proxy_client() {
        let _ = build_client(&ProxyConfig {
            mode: ProxyMode::Direct,
            url: "http://127.0.0.1:7890".into(),
        });
    }

    #[test]
    fn manual_mode_builds_client() {
        let _ = build_client(&ProxyConfig {
            mode: ProxyMode::Manual,
            url: "http://127.0.0.1:7890".into(),
        });
    }

    #[test]
    fn invalid_proxy_url_errors() {
        let err = build_client(&ProxyConfig {
            mode: ProxyMode::Manual,
            url: "not a valid url".into(),
        });
        assert!(err.is_err(), "非法代理地址应报错而不是静默忽略");
    }

    /// 起一个只服务有限次请求的本地 HTTP 服务（每连接一请求即关）。
    fn serve_once(responses: Vec<&'static str>) -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for resp in responses {
                let Ok((mut sock, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).unwrap();
                let _ = sock.write_all(resp.as_bytes());
            }
        });
        addr
    }

    /// 接一个请求、记下原始文本、回 200。返回给断言用。
    fn read_request_from(listener: std::net::TcpListener) -> String {
        use std::io::{Read, Write};
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = sock.read(&mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        let _ =
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        text
    }

    #[tokio::test]
    async fn video_stream_sends_bare_ua_first() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let observer = std::thread::spawn(move || read_request_from(listener));

        let url = format!("http://{addr}/v.mp4");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let resp = get_video_stream(&client, &url).await.expect("应成功");
        assert!(resp.status().is_success());

        let req = observer.join().unwrap();
        assert!(!req.contains("Referer"), "首次请求不应带 Referer: {req}");
        assert!(req.contains("GET /v.mp4"), "应请求指定路径: {req}");
        assert!(
            req.to_ascii_lowercase()
                .contains("accept-encoding: identity"),
            "视频流应禁用 HTTP 内容压缩，保持 MP4 原始字节: {req}"
        );
    }

    #[tokio::test]
    async fn video_range_requests_identity_encoding() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let observer = std::thread::spawn(move || read_request_from(listener));

        let url = format!("http://{addr}/v.mp4");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let bytes = get_video_range(&client, &url, 0, 1)
            .await
            .expect("测试服务器返回完整的两字节响应");
        assert_eq!(bytes, b"hi");

        let req = observer.join().unwrap().to_ascii_lowercase();
        assert!(
            req.contains("accept-encoding: identity"),
            "视频 Range 请求必须保持原始字节: {req}"
        );
        assert!(req.contains("range: bytes=0-1"), "应保留 Range: {req}");
    }

    #[tokio::test]
    async fn video_range_allows_a_slow_but_flowing_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).unwrap();
            let _ = sock.write_all(
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\n\
                  Content-Range: bytes 0-1/2\r\nConnection: close\r\n\r\n",
            );
            let _ = sock.flush();
            let _ = sock.write_all(b"h");
            let _ = sock.flush();
            std::thread::sleep(std::time::Duration::from_secs(1));
            let _ = sock.write_all(b"i");
        });

        let url = format!("http://{addr}/v.mp4");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let bytes = get_video_range(&client, &url, 0, 1)
            .await
            .expect("慢速但持续的响应应成功");
        assert_eq!(bytes, b"hi");
    }

    #[tokio::test]
    async fn video_stream_retries_with_referer_on_403() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = seen.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for is_403 in [true, false] {
                let Ok((mut sock, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap();
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                recorder.lock().unwrap().push(text);
                let (status, body) = if is_403 {
                    ("403 Forbidden", "")
                } else {
                    ("200 OK", "hi")
                };
                let _ = sock.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });

        let url = format!("http://{addr}/v.mp4");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let resp = get_video_stream(&client, &url)
            .await
            .expect("补 Referer 的重试应成功");
        assert!(resp.status().is_success());

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "应恰好发生两次请求");
        // http crate 会把头名转成小写发到线上，断言必须大小写不敏感
        assert!(
            !seen[0].to_ascii_lowercase().contains("referer"),
            "首次请求不应带 Referer"
        );
        assert!(
            seen[1].to_ascii_lowercase().contains("referer"),
            "403 后的重试必须带 Referer"
        );
        assert!(
            seen[1].contains(hongguo_signer::VIDEO_REFERER),
            "Referer 应是官网地址: {}",
            seen[1]
        );
    }

    #[tokio::test]
    async fn video_stream_reports_error_when_both_attempts_fail() {
        let addr = serve_once(vec![
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);
        let url = format!("http://{addr}/v.mp4");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let err = get_video_stream(&client, &url)
            .await
            .expect_err("两次都 403 应报错");
        assert!(
            err.to_string().contains("取流失败"),
            "错误信息应说明是取流失败: {err}"
        );
    }

    #[test]
    fn host_of_extracts_host() {
        assert_eq!(host_of("https://v.example.com/path?x=1"), "v.example.com");
        assert_eq!(host_of("not-a-url"), "not-a-url");
    }
}
