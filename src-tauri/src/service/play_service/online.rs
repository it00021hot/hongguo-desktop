//! 在线播放：取流 → 解密 → 推进内存流。
//!
//! 数据面在 [`crate::protocol::stream`]，这里只负责把明文推进去。
//!
//! ⚠️ 为什么不边下边解密：CENC 样本解密要先读 moov 里的样本表才能定位每个
//!    样本的字节区间，moov 又可能在文件尾部。所以现在先整集取回再解密，
//!    一次性交给 `serve`。**首帧要等整集下完**，比现版的渐进式播放慢，
//!    但结果字节与落盘下载完全一致。

use futures_util::StreamExt;

use crate::domain::model::{Settings, VideoDefinition};
use crate::error::{AppError, AppResult};
use crate::protocol::stream::StreamCache;

/// 在线播放就绪后带回的档位信息。
pub struct Prepared {
    pub url: String,
    /// 实际选中的档位。请求的档位平台没给时会回退，这里是回退后的真实值。
    pub definition: u32,
    /// 本集提供的全部档位
    pub definitions: Vec<VideoDefinition>,
}

/// 全局流缓存。
pub fn cache() -> &'static StreamCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<StreamCache> = OnceLock::new();
    CACHE.get_or_init(StreamCache::default)
}

/// 准备在线播放，返回流地址与档位信息。
///
/// 缓存按 (vid, 档位) 分条目（见 [`crate::protocol::stream`]），所以切清晰度
/// 既不用下载、也不会动到正在播的那一档：切回去直接命中内存里的旧数据。
pub async fn prepare(
    app: &tauri::AppHandle,
    vid: &str,
    definition: Option<u32>,
    settings: Settings,
) -> AppResult<Prepared> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    let c = cache();
    // 一次只看一集：把别的集从内存里清掉。不清的话整季 250 集会把内存吃光。
    c.keep_only(vid);

    // 不指定档位时用上一轮解析到的档位——缓存的键里必须有具体档位，
    // 否则无从查起。顺带让「拉分集」与「起播」两个并发请求认到同一条缓存，
    // 而不是各下一遍。
    if let Some(want) = definition.or_else(|| c.auto_definition(vid)) {
        if let Some(hit) = prepared(c, vid, want) {
            log::info!("[Online] {vid} 档位 {want} 已缓存，直接复用");
            return Ok(hit);
        }
        // 另一个请求正在把这一档取下来：等它，而不是自己也发一次下载。
        if c.is_fetching(vid, want) {
            log::info!("[Online] {vid} 档位 {want} 正在取流，等待复用");
            return wait_for_fetch(c, vid, want).await;
        }
    }

    // 还没有键可查：先做一次很小的接口调用把流表拿回来，解析出的真实档位
    // 才是缓存的键。贵的那一步（整集下载 + 解密）要等拿到取流权之后才发生。
    let play =
        crate::domain::api::play_url::fetch_play_url(vid, definition, &settings.proxy).await?;
    let want = play.definition;
    // 只记「不指定档位」那次解析到的结果。手动选过 720 之后再点「自动」，
    // 要的仍然是平台给的最高档，而不是上一次手动选的那档。
    if definition.is_none() {
        c.remember_auto(vid, want);
    }
    log::info!(
        "[Online] {vid} 选中档位 {want}（共 {} 档可选）",
        play.definitions.len()
    );

    if !c.begin_fetch(vid, want) {
        // 抢取流权时又被别人插队（窗口极窄），转为等待
        return wait_for_fetch(c, vid, want).await;
    }
    c.set_definitions(vid, want, &play.definitions);
    let url = crate::protocol::stream_url(vid, want);
    // play 整份 move 进后台填充任务，这里先把要回给调用方的字段取出来
    let result = Prepared {
        url,
        definition: want,
        definitions: play.definitions.clone(),
    };
    let owned = vid.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    let app = app.clone();
    tokio::spawn(async move {
        let filled = fill(&app, c, &owned, want, &play, &settings).await;
        // 取流权一直持有到数据落盘：中途放开会让并发的第二个请求以为
        // 「没人管这一档」而重新下一遍。
        c.end_fetch(&owned, want);
        if let Err(e) = filled {
            log::warn!("[Online] 填充 {owned} 失败: {e}");
            c.remove(&owned, want);
        }
    });

    Ok(result)
}

/// 缓存里这一档已经能供数时，组装出播放响应。
fn prepared(c: &StreamCache, vid: &str, definition: u32) -> Option<Prepared> {
    c.ready(vid, definition).map(|definitions| Prepared {
        url: crate::protocol::stream_url(vid, definition),
        definition,
        definitions,
    })
}

/// 等另一个请求把这一档取完，复用它的结果。
///
/// 播放页对同一集会并发发两次 `play_series`（拉分集 + 起播），第二次不再发一次
/// 下载（那会把整集白下一遍），而是等数据落进缓存。取流失败时对方会把条目删掉，
/// 这里据此判定「等不到」，按失败返回。
async fn wait_for_fetch(c: &StreamCache, vid: &str, definition: u32) -> AppResult<Prepared> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(20);
    // 上限要大于单次取流耗时（整集下载+解密），但别无限等：
    // 网络挂住时用户看到的是一条明确报错，而不是永远转圈。
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

    let start = std::time::Instant::now();
    loop {
        if let Some(hit) = prepared(c, vid, definition) {
            return Ok(hit);
        }
        // 对方失败并清掉了条目
        if !c.exists(vid, definition) {
            return Err(AppError::Network(format!("取流失败: {vid}")));
        }
        if start.elapsed() > DEADLINE {
            return Err(AppError::Network(format!("等待取流超时: {vid}")));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// 下载一集、解密、整段推进缓存。
async fn fill(
    app: &tauri::AppHandle,
    c: &StreamCache,
    vid: &str,
    definition: u32,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<()> {
    let reporter = ProgressReporter::new(app.clone(), vid.to_string());
    let plain =
        fetch_plain_with(play, settings, &|r, t, phase| reporter.report(r, t, phase)).await?;

    if !c.store(vid, definition, &plain) {
        log::info!(
            "[Online] {vid} 档位 {definition} 已有数据，丢弃重复的 {} 字节",
            plain.len()
        );
        reporter.done();
        return Ok(());
    }
    log::info!(
        "[Online] {vid} 档位 {definition} 就绪，{} 字节",
        plain.len()
    );
    Ok(())
}

/// 取一集并解密成明文字节（CDN 下载 + CENC 解密）。
///
/// 在线播放与兼容转码都要这一步，抽出来避免两份「取流 + 解密」逻辑各自漂移。
pub async fn fetch_plain(
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<Vec<u8>> {
    fetch_plain_with(play, settings, &|_, _, _| {}).await
}

/// 在线播放进度上报器。
///
/// 复用下载服务那套节流（0.5% 或 500ms 才发一次）：取流是每秒好几 MB 的量，
/// 每次都发会把 UI 线程打满。
///
/// 进度不是可选装饰：整集取回 + 解密期间界面上只有一个转圈，用户既看不出在动
/// 还是卡住，也看不到还要多久。
pub struct ProgressReporter {
    app: tauri::AppHandle,
    key: String,
    throttle: crate::service::download_service::events::ProgressThrottle,
}

impl ProgressReporter {
    pub fn new(app: tauri::AppHandle, key: String) -> Self {
        Self {
            app,
            key,
            throttle: Default::default(),
        }
    }

    /// 报一次进度。`total` 为 0（CDN 没给 Content-Length）时不发百分比。
    pub fn report(&self, received: u64, total: u64, phase: &str) {
        let percent = if total > 0 {
            received as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        if !self.throttle.should_send(&self.key, percent) {
            return;
        }
        use tauri::Emitter;
        let _ = self.app.emit(
            crate::service::download_service::events::names::ONLINE_PROGRESS,
            serde_json::json!({
                "key": self.key,
                "received": received,
                "total": total,
                "percent": percent.min(100.0),
                "phase": phase,
            }),
        );
    }

    /// 100%：解密完成、缓冲就绪。
    pub fn done(&self) {
        self.throttle.forget(&self.key);
        use tauri::Emitter;
        let _ = self.app.emit(
            crate::service::download_service::events::names::ONLINE_PROGRESS,
            serde_json::json!({
                "key": self.key,
                "received": 0,
                "total": 0,
                "percent": 100.0,
                "phase": "ready",
            }),
        );
    }
}

/// 带进度回调的取流+解密。
pub async fn fetch_plain_with(
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    progress: &(dyn Fn(u64, u64, &str) + Sync),
) -> AppResult<Vec<u8>> {
    let client = crate::domain::api::client::build_client(&settings.proxy)?;

    // CDN 对带 Referer 的请求直接 403，只带 App UA
    let resp = client
        .get(&play.url)
        .header("User-Agent", crate::signer::VIDEO_UA)
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "取流失败: HTTP {}",
            resp.status()
        )));
    }

    // 拿不到 Content-Length 就发不了百分比，但「开始动了」这件事仍要告诉界面
    let total = resp.content_length().unwrap_or(0);
    progress(0, total, "downloading");

    let mut body: Vec<u8> = Vec::with_capacity(total as usize);
    let mut received = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Network(e.to_string()))?;
        received += chunk.len() as u64;
        body.extend_from_slice(&chunk);
        progress(received, total, "downloading");
    }
    progress(received, total.max(received), "decrypting");

    if play.encrypted {
        let key = crate::domain::crypto::key_derive::derive_key(&play.key_material)?;
        crate::domain::mp4::decrypt_buffer::decrypt_mp4_buffer(&body, &key)
    } else {
        Ok(body)
    }
}

/// 清空在线缓存。
pub fn clear() -> usize {
    let (count, _) = cache().status();
    cache().clear();
    count
}
