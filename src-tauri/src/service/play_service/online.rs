//! 在线播放：取流 → 解密 → 推进内存流。
//!
//! 数据面在 [`crate::protocol::stream`]，这里只负责把数据推进去。
//!
//! 两条取流路径：
//! - **渐进**（首选）：并行预取头部与尾部 → 凭 moov 建解密计划 → 注册进协议层
//!   （此刻起即可应答 Range）→ 顺序填充余下字节。首帧只需等头 + 尾 + moov，
//!   不必等整集；seek 到未下载区段时按 seek 提示跳转填充。
//! - **整集**（回落）：CDN 不支持 Range、moov 定位失败等情形下，整集取回 +
//!   解密后一次性交给协议，行为与本功能加入前完全一致。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;

use crate::domain::model::{Settings, VideoDefinition};
use crate::error::{AppError, AppResult};
use crate::protocol::stream::{ProgressiveStream, StreamCache};

/// 头部预取量：覆盖 ftyp / mdat 头，让 demuxer 能算出 moov 位置。
const HEAD_BYTES: u64 = 512 * 1024;
/// 尾部预取量：moov 通常在文件末尾，2MB 足够装下常见短剧的 moov。
const TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// 顺序填充的步长：每段一个 Range 请求，太小请求开销大，太大等待粒度粗。
const FETCH_CHUNK: u64 = 4 * 1024 * 1024;

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
    env: &crate::domain::api::client::ApiEnv,
) -> AppResult<Prepared> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    let c = cache();
    // 一次只看一集：把别的集从内存里清掉。不清的话整季 250 集会把内存吃光。
    // 正在取流的豁免——那可能是沉浸流预取的下一部剧，清了就白取。
    c.keep_only_protect_fetching(vid);

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
        crate::domain::api::play_url::fetch_play_url(vid, definition, env).await?;
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

/// 预取一集的流（沉浸流「一切就下一部」的后台铺垫）。
///
/// 提前走完「取流表 + 建渐进流」这两步贵的前置，数据在后台渐进填充；
/// 用户真滚过去时 `prepare` 会命中 `ready`（渐进条目注册即就绪），
/// 立刻拿到流地址，首帧只等头部数据区间，不再黑屏干等。
///
/// 幂等：已有条目或已在取流时静默返回。失败同样静默——预取是锦上添花，
/// 报错只会把噪音推给根本没请求这件事的前端。
pub async fn prefetch_stream(
    app: &tauri::AppHandle,
    vid: &str,
    settings: &Settings,
    env: &crate::domain::api::client::ApiEnv,
) -> AppResult<()> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Ok(());
    }
    let c = cache();
    // 已解析过档位且条目在场/在取：无事可做
    if let Some(want) = c.auto_definition(vid) {
        if c.exists(vid, want) || c.is_fetching(vid, want) {
            return Ok(());
        }
    }
    let play = match crate::domain::api::play_url::fetch_play_url(vid, None, env).await {
        Ok(p) => p,
        Err(e) => {
            log::info!("[Online] 预取 {vid} 取流表失败（忽略）: {e}");
            return Ok(());
        }
    };
    let want = play.definition;
    c.remember_auto(vid, want);
    if c.is_fetching(vid, want) || c.exists(vid, want) {
        return Ok(());
    }
    if !c.begin_fetch(vid, want) {
        return Ok(());
    }
    c.set_definitions(vid, want, &play.definitions);
    log::info!("[Online] 预取 {vid} 档位 {want}，后台渐进填充开始");
    let owned = vid.to_string();
    let c = cache();
    let app = app.clone();
    let settings = settings.clone();
    tokio::spawn(async move {
        let filled = fill(&app, c, &owned, want, &play, &settings).await;
        c.end_fetch(&owned, want);
        if let Err(e) = filled {
            log::warn!("[Online] 预取填充 {owned} 失败: {e}");
            c.remove(&owned, want);
        }
    });
    Ok(())
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
///
/// 先尝试渐进路径（首帧不等整集）；计划构建失败才回落整集路径，
/// 回落后行为与旧版完全一致。注册成功后的填充阶段失败则直接上抛——
/// 那时回落只会把整集再重下一遍，和今天的失败语义一样交给上层清理。
async fn fill(
    app: &tauri::AppHandle,
    c: &StreamCache,
    vid: &str,
    definition: u32,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<()> {
    let reporter = ProgressReporter::new(app.clone(), vid.to_string());

    match build_progressive(play, settings, &|r, t| reporter.report(r, t, "downloading")).await {
        Ok((sparse, prog)) => {
            if !c.set_progressive(vid, definition, prog.clone()) {
                log::info!("[Online] {vid} 档位 {definition} 已有整集数据，渐进填充取消");
                reporter.done();
                return Ok(());
            }
            log::info!(
                "[Online] {vid} 档位 {definition} 渐进流就绪（密文 {} 字节），开始顺序填充",
                sparse.len()
            );
            // CDN 抖动（短读/断流）是常态而不是异常，整体重试而不是一次失败
            // 就永远停在前沿——播放追上未填充区只能超时报错。
            let mut last_err = None;
            for attempt in 0..3 {
                match fill_remaining(&sparse, &prog, play, settings, &reporter).await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        log::warn!(
                            "[Online] {vid} 档位 {definition} 填充第 {} 次中断: {e}",
                            attempt + 1
                        );
                        last_err = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }
            reporter.done();
            log::info!("[Online] {vid} 档位 {definition} 填充完成");
            Ok(())
        }
        Err(e) => {
            log::info!("[Online] {vid} 渐进取流不可用（{e}），回落整集路径");
            let plain = fetch_plain_with(play, settings, &|r, t, phase| {
                reporter.report(r, t, phase)
            })
            .await?;

            if !c.store(vid, definition, &plain) {
                log::info!(
                    "[Online] {vid} 档位 {definition} 已有数据，丢弃重复的 {} 字节",
                    plain.len()
                );
            }
            reporter.done();
            Ok(())
        }
    }
}

/// 渐进路径第一阶段：预取头尾、定位 moov、建计划、注册协议层。
///
/// 返回 `Err` 的所有情形都应回落整集路径（CDN 不支持 Range、找不到 moov、
/// 样本表解析失败……）。这一阶段完成时协议层已可应答，错误只会发生在
/// 「还没有任何对外可见状态」的时候，回落没有半成品要清理。
async fn build_progressive(
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    report: &(dyn Fn(u64, u64) + Sync),
) -> AppResult<(Arc<crate::domain::mp4::streaming::SparseBuffer>, Arc<ProgressiveStream>)> {
    use crate::domain::api::client::{get_video_range, probe_video_len};
    use crate::domain::mp4::streaming::{locate_moov, SparseBuffer, StreamingPlan};

    let client = crate::domain::api::client::build_client(&settings.proxy)?;
    let total = probe_video_len(&client, &play.url).await?;
    let sparse = SparseBuffer::new(total);

    // 头部：ftyp + mdat 头（demuxer 靠它算出 moov 在哪）
    let head_len = HEAD_BYTES.min(total);
    let head = get_video_range(&client, &play.url, 0, head_len - 1).await?;
    sparse.write(0, &head);
    report(sparse.downloaded(), total);

    // 尾部：moov 所在（平台流 moov 在尾部是常态）
    if total > head_len {
        let tail_len = TAIL_BYTES.min(total - head_len);
        let tail_from = total - tail_len;
        let tail = get_video_range(&client, &play.url, tail_from, total - 1).await?;
        sparse.write(tail_from, &tail);
        report(sparse.downloaded(), total);
    }

    // 定位 moov：先头部再尾部；定位到但没取全就补一段
    let located = locate_moov(
        &sparse_snapshot(&sparse, 0, head_len),
        0,
        total,
    )
    .or_else(|| {
        let tail_from = total - TAIL_BYTES.min(total.saturating_sub(head_len)).max(head_len);
        locate_moov(&sparse_snapshot(&sparse, tail_from, total), tail_from, total)
    });
    let Some((moov_start, moov_size)) = located else {
        return Err(AppError::Media("头尾预取里找不到 moov".into()));
    };
    if !sparse.covers(moov_start, moov_start + moov_size) {
        let extra = get_video_range(&client, &play.url, moov_start, moov_start + moov_size - 1)
            .await?;
        sparse.write(moov_start, &extra);
        report(sparse.downloaded(), total);
    }
    let region = sparse_snapshot(&sparse, moov_start, moov_start + moov_size);

    let prog = if play.encrypted {
        let key = crate::domain::crypto::key_derive::derive_key(&play.key_material)?;
        let plan = StreamingPlan::build(&region, moov_start, total, &key)?;
        let plain_len = plan.plain_len();
        Arc::new(ProgressiveStream {
            sparse: sparse.clone(),
            plan: Some(Arc::new(plan)),
            plain_len,
            seek_hint: AtomicU64::new(0),
        })
    } else {
        // 明文流（官网兜底链路）：明文就是密文，按需直供
        Arc::new(ProgressiveStream {
            sparse: sparse.clone(),
            plan: None,
            plain_len: total,
            seek_hint: AtomicU64::new(0),
        })
    };
    Ok((sparse, prog))
}

/// 渐进路径第二阶段：把余下字节按序填满，期间响应 seek 提示。
async fn fill_remaining(
    sparse: &Arc<crate::domain::mp4::streaming::SparseBuffer>,
    prog: &Arc<ProgressiveStream>,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    reporter: &ProgressReporter,
) -> AppResult<()> {
    use crate::domain::api::client::get_video_range;

    let client = crate::domain::api::client::build_client(&settings.proxy)?;
    let total = sparse.len();
    let mut cursor = 0u64;
    loop {
        // seek 提示在密文上落在更前方时跳过去；跳过的洞由回绕补齐
        let hint_plain = prog.seek_hint.load(Ordering::Acquire);
        let hint_cipher = match &prog.plan {
            // 尾部-moov 布局下 plain == cipher，头部布局按计划映射一次
            Some(p) => p
                .cipher_ranges_needed(hint_plain, hint_plain + 1)
                .first()
                .map(|(a, _)| *a)
                .unwrap_or(hint_plain),
            None => hint_plain,
        };
        if hint_cipher > cursor {
            cursor = hint_cipher;
        }

        let Some((gap_start, gap_end)) = sparse.next_gap(cursor) else {
            if sparse.downloaded() >= total {
                return Ok(());
            }
            cursor = 0; // cursor 之后全满：回绕找剩下的洞
            continue;
        };
        let fetch_end = gap_start + FETCH_CHUNK.min(gap_end - gap_start);
        let bytes = get_video_range(&client, &play.url, gap_start, fetch_end - 1).await?;
        let want = fetch_end - gap_start;
        let got = bytes.len() as u64;
        if got == 0 {
            // 空响应意味着 write 不会标记任何覆盖，cursor 又已越过这里——
            // 只能靠回绕反复重试，等于静默空转。直接报错让上层看见。
            return Err(AppError::Network(format!(
                "CDN 区间 {gap_start}-{} 返回空响应",
                fetch_end - 1
            )));
        }
        if got != want {
            log::info!("[Online] 填充区间 {gap_start}-{fetch_end} 短读：要 {want} 字节拿到 {got}");
        }
        sparse.write(gap_start, &bytes);
        let done = sparse.downloaded();
        log::debug!("[Online] 填充区间 {gap_start}-{fetch_end} 完成（累计 {done}/{total}）");
        reporter.report(done, total, "downloading");
        cursor = fetch_end;
    }
}

/// 从稀疏缓冲拷出一段（调用方保证已覆盖）。
fn sparse_snapshot(
    sparse: &crate::domain::mp4::streaming::SparseBuffer,
    start: u64,
    end: u64,
) -> Vec<u8> {
    let mut out = vec![0u8; (end - start) as usize];
    sparse.read_into(start, &mut out);
    out
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

    // 首选裸 UA；个别 CDN 节点反过来要求 Referer，403 时在里面补上重试
    let resp = crate::domain::api::client::get_video_stream(&client, &play.url).await?;

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
