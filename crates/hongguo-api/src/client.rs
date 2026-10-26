//! 接口调用：重试与空响应判定。
//!
//! ⚠️ 服务端在签名失效时返回 **HTTP 200 + 0 字节**，不是错误码。
//! 只看状态码会把失败误判成成功，所以这里显式检查响应体长度。
//!
//! 两条与签名强相关、不能搞错的约定：
//!
//! 1. **方法必须与签名时一致。** 签名覆盖 body 字节，带 body 的请求发成
//!    `GET` 会让服务端算出的摘要与 `x-gorgon` / `x-ss-stub` 不符，直接静默丢弃。
//! 2. **每次重试都要重新签名。** 时间戳与 `_rticket` 已经过期，复用旧签名
//!    等于拿一张过期的票去敲门。

use std::time::Duration;

use reqwest::Proxy;

use hongguo_common::{AppError, AppResult};

/// 重试次数。
const MAX_RETRIES: u32 = 3;

/// reading 系（lq 域）统一 origin。轻签名头族（榜单/搜索/弹幕/剧评）与
/// 取流 `multi_video_model/player/v1` 都在这台机上——2026-10-08 抓包：
/// 真实 App 的取流请求打的是 lq 域，sinfonlineb 域的 player/preload 均
/// 回 200+0 字节（2026-10-14 探针实测）。
pub const LQ_API_ORIGIN: &str = "https://api5-normal-lq.fqnovel.com";

/// 重试间隔基数，按 `2s * (i + 1)` 递增，与现版一致。
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);

/// CDN 取流重试次数（**失败重试 3 次，退避 300ms×(第几次)**——CDN 抖动是
/// 常态）。本文件的 Range 自愈与 service 的在线填充两侧同源，改这里一处。
pub const STREAM_RETRY_ATTEMPTS: u32 = 3;

/// 第 `attempt`（0 基）次失败后的退避时长（`it()` 口径）。
pub fn stream_retry_backoff(attempt: u32) -> Duration {
    Duration::from_millis(300 * (attempt as u64 + 1))
}

/// 按配置取 HTTP client（进程级按解析后的代理地址缓存）。
///
/// `reqwest::Client` 自带连接池，**每次新建等于把 TLS 握手/代理连接全部
/// 付一遍**——预约页翻页、榜单连打这类连续请求被整体放大（issue #11 的
/// 「已上线 5–8 秒」里相当一块在这）。缓存键是解析后的代理地址：用户改
/// 代理设置即得到新 client（旧连接池随引用释放），免重启；构造失败不缓存，
/// 下次调用重试。
pub fn build_client(config: &hongguo_common::model::ProxyConfig) -> AppResult<reqwest::Client> {
    static CLIENTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, reqwest::Client>>,
    > = std::sync::OnceLock::new();
    let key = config.resolved().unwrap_or_default();
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

/// 真构造（`build_client` 的缓存未命中分支）：UA 与超时是全局契约，
/// 不随代理配置变。
fn build_client_uncached(
    config: &hongguo_common::model::ProxyConfig,
) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(hongguo_signer::video_ua())
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    match config.resolved() {
        Some(url) => {
            let proxy =
                Proxy::all(&url).map_err(|e| AppError::Network(format!("代理地址无效: {e}")))?;
            builder = builder.proxy(proxy);
        }
        None => {
            builder = builder.no_proxy();
        }
    }

    builder
        .build()
        .map_err(|e| AppError::Network(format!("构造 HTTP client 失败: {e}")))
}

/// 一次官方 API 调用的全部环境：代理、设备档案、会话 Cookie。
///
/// 三者都从 `AppState` 快照而来（应用壳的 `AppStateInner::api_env`），
/// 调用链上以值传递，避免中途被设置修改后出现「签名用 A 设备、请求带 B Cookie」
/// 之类的半新半旧状态。
#[derive(Debug, Clone)]
pub struct ApiEnv {
    pub proxy: hongguo_common::model::ProxyConfig,
    pub device: hongguo_signer::device::DeviceProfile,
    /// 会话 Cookie（`k=v; k=v` 形态）。未登录为 None。
    /// Cookie 不参与签名，走 `extra_headers` 注入。
    pub cookie: Option<String>,
    /// x-tt-token 长凭据（登录响应头下发；在场则以短形式随全部请求头
    /// 携带——这也是将来服务端滑动续期时新值的载体）
    pub x_tt_token: Option<String>,
}

impl ApiEnv {
    /// 未登录的默认环境：匿名游客档案（官方 7.4.2.32 形态，无设备身份）
    /// + 默认 origin。
    ///
    /// 测试基线专用（生产走登录态 / 注册档案）。不 `#[cfg(test)]` 门控：下游
    /// crate（本仓 crates/hongguo-service 的测试）要用，而依赖 crate 对下游以非
    /// test 方式编译，门控项会消失。
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
///
/// # 参数
/// - `pathname`：以 `/` 开头的接口路径
/// - `body`：请求体字节。`Some` 走 POST，`None` 走 GET
/// - `env`：代理 / 设备 / 会话
pub async fn api_call(pathname: &str, body: Option<Vec<u8>>, env: &ApiEnv) -> AppResult<Vec<u8>> {
    api_call_full(hongguo_signer::API_ORIGIN, pathname, body, &[], env).await
}

/// 全参数形态：origin + 业务 query + body。reading 系接口是混合参数
/// （部分参数在 query、部分在 body 的 biz_param），都从这里走。
pub async fn api_call_full(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    api_call_full_with_headers(origin, pathname, body, biz_query, &[], env).await
}

/// [`api_call_full`] 的带额外头版本（commentapi 要 `x-reading-request`）。
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

/// 单次发送、空响应不重试不报错（回空 body）。批量取流预取专用：
/// 2026-10-14 起批量形态恒回 200+0（服务端不再发货），它是「发了即回」
/// 的常态而非故障——重试只会让每次切剧白等 6 秒，一次即回。
pub async fn api_call_once_allow_empty(
    origin: &str,
    pathname: &str,
    body: Option<Vec<u8>>,
    biz_query: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Vec<u8>> {
    let client = build_client(&env.proxy)?;
    let resp = send_once(
        &client,
        origin,
        pathname,
        body.as_deref(),
        biz_query,
        &[],
        env,
    )
    .await?;
    Ok(resp.bytes)
}

/// 响应体 + Set-Cookie（passport 登录捕获会话用，其余调用走
/// [`api_call_full_with_headers`] 自动丢弃 cookie）。
pub struct ApiCallResponse {
    pub bytes: Vec<u8>,
    /// Set-Cookie 原始行（`k=v; Path=/; ...` 形态，未清洗）。
    pub set_cookies: Vec<String>,
    /// 全部响应头（小写名）。目前只为提取 `x-tt-token`（登录响应头
    /// 下发的长凭据，Set-Cookie / body 里都没有）。
    pub headers: Vec<(String, String)>,
}

/// reading 系（lq 域）统一调用入口：轻签名头 + POST body 一律 gzip。
///
/// 1.1.3 全量抓包对齐：这族接口的 POST（弹幕/预约/筛选）全部
/// `Content-Encoding: gzip`，GET/POST 都带 `x-ss-dp + lc +
/// x-reading-request`。走 sinfonlineb 域的播放/推荐流是项目原有
/// 验证形态（未压缩 + 全签名），不走这里。
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

/// reading 系接口的**原样 body** 变体：body 不 gzip，`Content-Type` 由调用方
/// 指定（图片上传的 `multipart/form-data; boundary=…` 用）。
///
/// 签名侧照常按原始 body 算摘要（`x-ss-stub`），与官方 App 实包一致——
/// 官方上传的 multipart body 也是原样参与签名。
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

/// gzip 压缩（reading 系 POST body 的线上形态）。
pub fn gzip_bytes(raw: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(raw)
        .and_then(|_| encoder.finish())
        .map_err(|e| AppError::Network(format!("gzip 压缩失败: {e}")))
}

/// reading 系接口的轻签名头（search / commentapi / 预约共用）。
///
/// 抓包实证：这族接口不带 x-gorgon/x-argus/x-ladon，只要
/// `x-ss-dp + x-reading-request + lc`（多带签名头也能过）。
/// `x-reading-request` 形如 `{ticket_ms}-{random u32}`。
pub fn reading_headers() -> Vec<(String, String)> {
    /// x-ss-dp：红果 App 的 aid（与 docs 抓包对齐）。
    const READING_DP: &str = "8662";
    /// lc：reading 系固定头（抓包值 101，语义上游未公开）。
    const LC: &str = "101";
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
                last_err = "接口返回空响应（签名可能失效）".to_string();
            }
            Err(e) => last_err = e.to_string(),
        }
        if attempt + 1 < MAX_RETRIES {
            tokio::time::sleep(RETRY_BASE_DELAY * (attempt + 1)).await;
        }
    }

    Err(AppError::EmptyResponse(last_err))
}

/// 请求视频 CDN 直链：只带 App UA，被 403 拒时补官网 Referer 重试一次。
///
/// 下载落盘（worker）与在线取流（play_service）都走这里，两边口径必须一致：
/// 防盗链规则在同一时刻只有一种，一边能过一边过不了只能是实现漂移。
pub async fn get_video_stream(client: &reqwest::Client, url: &str) -> AppResult<reqwest::Response> {
    let send = |referer: bool| {
        let mut req = client
            .get(url)
            .header("User-Agent", hongguo_signer::video_ua())
            // MP4 已经压缩，HTTP gzip 会破坏 Range 偏移并可能让 reqwest
            // 在读响应体时解压失败。视频必须按原始字节收取。
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

/// 从 URL 里抠出主机名，失败就原样返回——只用于错误信息，不值得为它引入
/// 完整的 URL 解析错误处理。
fn host_of(url: &str) -> &str {
    url.split("//")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .unwrap_or(url)
}

// ---------------------------------------------------------------- 视频 Range 请求

/// 探测请求的总超时。只取 1 字节，健康 CDN 秒级；卡死连接不该吃 client 默认 30s。
const VIDEO_RANGE_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// 等响应头的上限。真正卡死的连接通常卡在这里，不能等到整个 body 超时。
const VIDEO_RANGE_HEADER_TIMEOUT: Duration = Duration::from_secs(15);

/// 读取停滞超时：连续这么久没有**任何**新字节就报错。
///
/// 这是「卡死连接」和「慢但仍在传」的分界。不能用总耗时（1MB 在 68KB/s
/// 以下会误杀健康慢网）；只要每 10s 有推进，就继续读。
const VIDEO_RANGE_STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// 单次 Range 请求的总上限（兜底防「每 9s 来一个字节」的假活跃连接）。
/// 正常的 1MB 块远小于它。
const VIDEO_RANGE_TOTAL_TIMEOUT: Duration = Duration::from_secs(120);

/// 用 `Range: bytes=0-0` 探测直链总长。
///
/// 返回 `Err` 表示该 CDN 不支持 Range（回了 200）——流式取流的前提不成立，
/// 调用方据此回落整集路径。
pub async fn probe_video_len(client: &reqwest::Client, url: &str) -> AppResult<u64> {
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

/// 取直链的一段密文 `[start, end]`（闭区间，按字节计；`Range: bytes=start-end`）。
///
/// 与 [`get_video_stream`] 同一套头（裸 UA 优先、403 补 Referer）。
/// 非 206 一律按不支持 Range 报错，交给调用方回落。
///
/// 取流重试口径：
/// - **失败重试 3 次，退避 300ms×(第几次)**——CDN 抖动是常态；
/// - **CDN 忽略 Range 回 200 全量时切片自愈**而不是报错：按已收字节量
///   跳过前缀、只留 Range 请求的数据段。再多做一步——
///   边收边丢、收够即断，不为切 1MB 白拉整集；
/// - 个别节点 403 裸 UA，补 Referer 重试一次（本应用独有的节点差异）。
pub async fn get_video_range(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    end: u64,
) -> AppResult<Vec<u8>> {
    let mut last_err = None;
    for attempt in 0..STREAM_RETRY_ATTEMPTS {
        match get_video_range_once(client, url, start, end).await {
            Ok(bytes) => return Ok(bytes),
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
        // 206：响应体就是请求的那段（短读语义由调用方处理）
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
        // 200：CDN 忽略了 Range 回全量——边收边切，收满要的区间就断流
        let mut cutter = RangeCutter::new(start, end);
        loop {
            match tokio::time::timeout(VIDEO_RANGE_STALL_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    if cutter.push(&chunk) {
                        break; // 收够了，剩余字节不要了（提前断开连接）
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

/// 200 全量响应里切 `[start, end]` 的状态机（`push` 返回 true = 收够了）。
///
/// 独立成结构体是为了能单测：跳过 `start` 之前、取满 `end-start+1` 即止，
/// 与到达顺序/分块边界无关。
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

    /// 取出切好的区间。全量响应比请求区间短时给到有多少（短读语义），
    /// 一个字节都没落到说明 start 越界，按错误处理。
    fn finish(self) -> AppResult<Vec<u8>> {
        if self.out.is_empty() {
            return Err(AppError::Network("200 全量响应未覆盖请求区间的起点".into()));
        }
        Ok(self.out)
    }
}

/// 发一次请求。签名在此处生成，与请求方法在同一个分支里决定，不会错配。
///
/// Cookie 不参与签名（字节面只覆盖 query + body + 时间戳），作为 extra_headers
/// 在签名完成后注入——顺序上先签名后补 Cookie，不会被签进摘要里。
async fn send_once(
    client: &reqwest::Client,
    origin: &str,
    pathname: &str,
    body: Option<&[u8]>,
    biz_query: &[(String, String)],
    extra_headers: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<ApiCallResponse> {
    let mut extra: Vec<(String, String)> = extra_headers.to_vec();
    if let Some(c) = &env.cookie
        && !c.is_empty()
    {
        extra.push(("Cookie".into(), c.clone()));
    }
    // x-tt-token：登录响应头下发的凭据，短形式（前 56 位）随请求带——
    // 业务请求一律在场（2026-10-05 抓包对齐；也是服务端
    // 滑动续期时新值的下发载体）
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
        .map_err(|e| AppError::Network(e.to_string()))?;
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
        .map_err(|e| AppError::Network(e.to_string()))?;
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
        // 200 全量 [0..100)，要 [30..49]：跨多个分块喂入，收满即止
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
        // 全量响应比请求区间短：给到有多少（短读语义由调用方处理）
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
        // 常量在编译期已知，这条断言的作用是「改动时有人会看见」
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

    #[test]
    fn client_builds_for_default_proxy() {
        // 探针与测试都走默认代理，构造成功即说明配置可用
        let _ = build_client(&ProxyConfig::default());
    }

    #[test]
    fn direct_mode_builds_no_proxy_client() {
        // 构造成功即说明直连配置被接受
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

    // ---------------------------------------------------------------- 视频 CDN 直链

    /// 起一个只服务有限次请求的本地 HTTP 服务。
    ///
    /// 每个连接只读一个请求、回一个写死的响应就关连接（`Connection: close`，
    /// 强制客户端为下一次请求新开连接——否则连接复用会让「第二个请求」
    /// 仍然落在第一条 TCP 上，测试就等不到第二次 accept）。
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
        // 第一个连接只用来观察请求头：不应带 Referer（带了会被主流 CDN 边缘 403）
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
        // 慢但一直在传：每 1s 到 1 字节，不能被总耗时超时误杀（停滞超时 10s）。
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
        // 服务端：第一次裸 UA → 403；第二次带 Referer → 200。
        // 两边各记下收到的请求头，验证「先裸、后补」的顺序。
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
