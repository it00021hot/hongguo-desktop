//! 在线播放：取流 → 解密 → 推进内存流。
//!
//! 数据面在 [`crate::protocol::stream`]，这里只负责把数据推进去。
//!
//! 两条取流路径：
//! - **渐进**（首选）：首探头部 256KB → 盒游走定位 moov → 精确补齐 → 建
//!   解密计划 → 注册进协议层（此刻起即可应答 Range）→ 顺序填充余下字节
//!   （探针序列对齐 hgplayer 取流 worker）。seek 到未下载区段时由读者需求
//!   登记（ReaderDemand）驱动优先填充，回跳与前进一视同仁。
//! - **整集**（回落）：CDN 不支持 Range、moov 定位失败等情形下，整集取回 +
//!   解密后一次性交给协议，行为与本功能加入前完全一致。

use std::sync::Arc;

use futures_util::StreamExt;

use crate::domain::model::{Settings, VideoDefinition};
use crate::error::{AppError, AppResult};
use crate::protocol::stream::{ProgressiveStream, StreamCache};

/// 头部首探量，对齐 hgplayer 的 `it(0, 262144)`：ftyp + mdat 头都装得下，
/// 顶层盒表从这里算出 moov 落点。
const HEAD_BYTES: u64 = 256 * 1024;
/// moov 不在首探里时的盒游走步进，对齐 hgplayer 的 `it(f, f+64*1024)`：
/// 从「最后一个顶层盒的结束处」起探。
const BOX_WALK_STEP: u64 = 64 * 1024;
/// 盒游走最多轮数，对齐 hgplayer `Me()` 的 `for(i<4)`：正常尾-moov 布局
/// 第一轮就命中，多轮只防非常规盒序（fragmented 之类）。
const BOX_WALK_MAX: usize = 4;
/// 顺序填充的步长：每段一个 Range 请求。
///
/// 1MB，对齐 hgplayer（第三方 Go+Wails 客户端）实测值——其前端 worker 按
/// mp4 样本表分组拉流，单请求上限 `1<<20`（首探 256KB、盒游走 64KB 步进；
/// 见 captures/hgplayer-116-frontend.js 的取流 worker）。分块越小，
/// seek 需求打断顺序填充的粒度越细，回跳响应越快。
const FETCH_CHUNK: u64 = 1024 * 1024;

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

/// 填充门闸：同时只允许一路「下载+解密」。
///
/// 拿着门闸时才真正开始下载；等门闸的调用方在 `begin_fetch` 已登记取流权，
/// 不会重复发请求。放在 fill 开头 acquire、函数结束自动释放。
static FILL_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// 准备在线播放，返回流地址与档位信息。
///
/// 缓存按 (vid, 档位) 分条目（见 [`crate::protocol::stream`]），所以切清晰度
/// 既不用下载、也不会动到正在播的那一档：切回去直接命中内存里的旧数据。
pub async fn prepare(
    app: &tauri::AppHandle,
    vid: &str,
    progress_key: &str,
    definition: Option<u32>,
    settings: Settings,
    env: &crate::domain::api::client::ApiEnv,
) -> AppResult<Prepared> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    let c = cache();
    // 记下当前集：逐出策略保它、预填礼让它、切走后旧填充据此自杀
    c.set_current(vid);
    // 一次只看一集的**语义下有界保留**：当前集全部档位 + 显式预取 + 最近几条。
    // 全清的话，信息流切剧目标一弹，正在被 <video> 读的那条流就被逐掉，
    // 播放追上缓冲前沿就是 30s 超时 + MediaError（「视频处理失败」根因）。
    c.retain_playing_set(vid);

    // 不指定档位时先认上一轮解析到的档位——缓存的键里必须有具体档位，
    // 否则无从查起。连 auto 记录都没有（重启后第一次、预取还没走到），
    // 就认领同 vid 任一已就绪的条目：有现成的流，别把取流表+建流+填头部
    // 整条链路再走一遍。顺带让「拉分集」与「起播」两个并发请求认到同一条缓存。
    let want = match definition.or_else(|| c.auto_definition(vid)) {
        Some(w) => Some(w),
        None => c.ready_any(vid).map(|(d, _)| {
            c.remember_auto(vid, d);
            d
        }),
    };
    if let Some(want) = want {
        if let Some(hit) = prepared(c, vid, want) {
            // 预取只填了头部的条目：转正后立刻续填余下（后台，不挡播放）
            resume_fill(app, vid, want, progress_key);
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
    let play = crate::domain::api::play_url::fetch_play_url(vid, definition, env).await?;
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
    let owned_key = progress_key.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    let app = app.clone();
    tokio::spawn(async move {
        let filled = fill(&app, c, &owned, &owned_key, want, &play, &settings, false).await;
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
    progress_key: &str,
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
    // 显式标记预取：keep_only 只豁免这一条，别让在途填充占住串行队列
    c.mark_prefetch(vid, want);
    log::info!("[Online] 预取 {vid} 档位 {want}，后台渐进填充开始");
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    let c = cache();
    let app = app.clone();
    let settings = settings.clone();
    tokio::spawn(async move {
        let filled = fill(&app, c, &owned, &owned_key, want, &play, &settings, true).await;
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
///
/// **全局串行**（[`FILL_GATE`]）：当前剧与预取的填充排队走，一次只填一路。
/// 并发两路整集下载+解密会抢爆带宽与 CPU——用户实测卡死的那次，
/// 当前剧与预取同时 fill 是最可疑的现场。
///
/// **可取消**（2026-10-06）：条目被逐出（用户切走）后填充立即中止让出队列——
/// 否则被切走的前一集拖着整集下载占住串行通道，当前集黑屏干等到天荒地老。
/// 每个分块边界都检查一次，最坏滞后一个分块（4MB）。
///
/// `prefetch = true` 时是**有界**预填（默认只填 [`PREFETCH_HEAD_BYTES`]，
/// 够首帧秒开），但上限是动态的：条目升格为当前集、或已有读者在等这一档，
/// 上限即刻解除、同一条填充无缝续满——不必依赖 [`resume_fill`] 事后抢
/// 取流权（预取尚未收工时它抢不到，那正是「播到头部上限就断流」的窗口）。
#[allow(clippy::too_many_arguments)]
async fn fill(
    app: &tauri::AppHandle,
    c: &StreamCache,
    vid: &str,
    progress_key: &str,
    definition: u32,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    prefetch: bool,
) -> AppResult<()> {
    // 让位规则（不只看逐出）：用户滚走后条目虽被保留集留下（数据要保住，
    // 弹回来秒切），但它的**下载**必须立即让位——串行队列只有一路，
    // 被切走剧的整集下载拖住队头，当前剧就只能干等（「连滚几部后加载
    // 很久」的根因）。预取（有界头部）除外。
    let cancelled = || {
        !c.exists(vid, definition)
            || (!prefetch && c.current() != vid && !c.is_prefetch_marked(vid, definition))
    };
    let _gate = FILL_GATE.acquire().await;
    if cancelled() {
        log::info!("[Online] {vid} 已被切走/让位，排队轮到时取消填充");
        return Ok(());
    }
    let reporter = ProgressReporter::new(app.clone(), progress_key.to_string());

    match build_progressive(play, settings, &|r, t| reporter.report(r, t, "downloading")).await {
        Ok((sparse, prog)) => {
            if cancelled() || !c.set_progressive(vid, definition, prog.clone()) {
                log::info!("[Online] {vid} 档位 {definition} 条目已失效，渐进填充取消");
                reporter.done();
                return Ok(());
            }
            log::info!(
                "[Online] {vid} 档位 {definition} 渐进流就绪（密文 {} 字节），开始顺序填充",
                sparse.len()
            );
            // CDN 抖动（短读/断流）是常态而不是异常，整体重试而不是一次失败
            // 就永远停在前沿——播放追上未填充区只能超时报错。
            // 预取的动态上限：升格为当前集或已有读者在等即解除（见 fill 文档）
            let cap: Box<dyn Fn() -> Option<u64> + Send + Sync> = if prefetch {
                let key = vid.to_string();
                let prog_cap = prog.clone();
                Box::new(move || {
                    if cache().current() == key || prog_cap.demand.has_waiters() {
                        None
                    } else {
                        Some(PREFETCH_HEAD_BYTES)
                    }
                })
            } else {
                Box::new(|| None)
            };
            let mut last_err = None;
            for attempt in 0..3 {
                if cancelled() {
                    log::info!("[Online] {vid} 已被切走，中止填充");
                    return Ok(());
                }
                match fill_remaining(&sparse, &prog, play, settings, &reporter, &cancelled, &cap)
                    .await
                {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        if cancelled() {
                            return Ok(());
                        }
                        log::warn!(
                            "[Online] {vid} 档位 {definition} 填充第 {} 次中断: {e}",
                            attempt + 1
                        );
                        last_err = Some(e);
                        // 退避对齐 hgplayer 取流 worker 的 it()：300ms×(第几次)
                        let backoff = std::time::Duration::from_millis(300 * (attempt as u64 + 1));
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }
            reporter.done();
            log::info!(
                "[Online] {vid} 档位 {definition} {}完成",
                if prefetch { "预取头部 " } else { "填充 " }
            );
            Ok(())
        }
        Err(e) => {
            log::info!("[Online] {vid} 渐进取流不可用（{e}），回落整集路径");
            let plain =
                fetch_plain_with(play, settings, &|r, t, phase| reporter.report(r, t, phase))
                    .await?;

            if cancelled() {
                return Ok(());
            }
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

/// 预取只填的头部字节数：够 `<video>` 首帧 + 前几十秒，不背整集。
/// 升格为当前集后由 [`resume_fill`] 续满。
const PREFETCH_HEAD_BYTES: u64 = 8 * 1024 * 1024;

/// 预取条目升格为当前集后的**续填**：渐进已注册、URL/代理存在条目里，
/// 直接把余下字节按序填满（当前集优先级，无窗口上限、不礼让）。
///
/// 没有它，预取只填头部的条目在被切上后，播放追上填充前沿就只能
/// 30s 超时——「滚回来就黑屏」的另一半。
pub fn resume_fill(app: &tauri::AppHandle, vid: &str, definition: u32, progress_key: &str) {
    let c = cache();
    if !c.needs_resume(vid, definition) {
        return;
    }
    if !c.begin_fetch(vid, definition) {
        return; // 已有填充方
    }
    let Some((sparse, prog)) = c.resume_parts(vid, definition) else {
        c.end_fetch(vid, definition);
        return;
    };
    let proxy = prog.proxy.clone();
    let cdn_url = prog.cdn_url.clone();
    let app = app.clone();
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    tokio::spawn(async move {
        let reporter = ProgressReporter::new(app.clone(), owned_key);
        let cancelled = || !c.exists(&owned, definition) || c.current() != owned;
        let client = match crate::domain::api::client::build_client(&proxy) {
            Ok(cl) => cl,
            Err(e) => {
                log::warn!("[Online] 续填 {owned} 建 client 失败: {e}");
                c.end_fetch(&owned, definition);
                return;
            }
        };
        let result = fill_remaining_with_client(
            &client,
            &sparse,
            &prog,
            &cdn_url,
            &reporter,
            &cancelled,
            &|| None, // 续填只发生在当前集上：无上限，填满为止
        )
        .await;
        c.end_fetch(&owned, definition);
        match result {
            Ok(()) => log::info!("[Online] {owned} 续填完成"),
            Err(e) => log::warn!("[Online] {owned} 续填中断（条目保留，可播已填部分）: {e}"),
        }
    });
}

/// 渐进路径第一阶段：首探头部、盒游走定位 moov、精确补齐、建计划、注册协议层。
///
/// 探针序列对齐 hgplayer worker 的 `Me()`（见 captures/hgplayer-116-frontend.js）：
/// 首探 256KB → 首探里没 moov 就从「最后一个顶层盒的结束处」64KB 步进游走
/// （最多 4 轮）→ 精确区间拉 moov 本体。**不做固定尾部预取**——多下的字节
/// 在 moov 拉完前纯属浪费首帧时间；尾部数据的即时性由读者需求登记保证。
///
/// 返回 `Err` 的所有情形都应回落整集路径（CDN 不支持 Range、找不到 moov、
/// 样本表解析失败……）。这一阶段完成时协议层已可应答，错误只会发生在
/// 「还没有任何对外可见状态」的时候，回落没有半成品要清理。
async fn build_progressive(
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    report: &(dyn Fn(u64, u64) + Sync),
) -> AppResult<(
    Arc<crate::domain::mp4::streaming::SparseBuffer>,
    Arc<ProgressiveStream>,
)> {
    use crate::domain::api::client::{get_video_range, probe_video_len};
    use crate::domain::mp4::streaming::{locate_moov, top_boxes_end, SparseBuffer, StreamingPlan};

    let client = crate::domain::api::client::build_client(&settings.proxy)?;
    let total = probe_video_len(&client, &play.url).await?;
    let sparse = SparseBuffer::new(total);

    // 首探 256KB：ftyp + mdat 头，顶层盒表从这里算出 moov 落点
    let head_len = HEAD_BYTES.min(total);
    let head = get_video_range(&client, &play.url, 0, head_len - 1).await?;
    sparse.write(0, &head);
    report(sparse.downloaded(), total);

    // moov 定位：先看首探（head 布局），没有就盒游走（tail 布局常态第一轮命中）
    let mut located = locate_moov(&head, 0, total);
    if located.is_none() {
        let mut walk_from = top_boxes_end(&head, 0);
        for _ in 0..BOX_WALK_MAX {
            let Some(from) = walk_from else { break };
            if from >= total {
                break;
            }
            let seg_end = (from + BOX_WALK_STEP).min(total);
            let seg = get_video_range(&client, &play.url, from, seg_end - 1).await?;
            // 游走段是真实文件字节，落进稀疏缓冲不浪费
            sparse.write(from, &seg);
            report(sparse.downloaded(), total);
            if let Some(found) = locate_moov(&seg, from, total) {
                located = Some(found);
                break;
            }
            // 这段里没有：从本段最后一个顶层盒的结束处继续走
            match top_boxes_end(&seg, from) {
                Some(next) if next > from => walk_from = Some(next),
                _ => break,
            }
        }
    }
    let Some((moov_start, moov_size)) = located else {
        return Err(AppError::Media("首探与盒游走都没找到 moov".into()));
    };
    // 精确补齐 moov 本体（首探/游走段只保证拿到它的头）
    if !sparse.covers(moov_start, moov_start + moov_size) {
        let extra =
            get_video_range(&client, &play.url, moov_start, moov_start + moov_size - 1).await?;
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
            demand: Default::default(),
            cdn_url: play.url.clone(),
            proxy: settings.proxy.clone(),
        })
    } else {
        // 明文流（官网兜底链路）：明文就是密文，按需直供
        Arc::new(ProgressiveStream {
            sparse: sparse.clone(),
            plan: None,
            plain_len: total,
            demand: Default::default(),
            cdn_url: play.url.clone(),
            proxy: settings.proxy.clone(),
        })
    };
    Ok((sparse, prog))
}

/// 渐进路径第二阶段：把余下字节填满，期间响应读者需求。
///
/// [`fill_remaining_with_client`] 是三级调度器：需求优先 → 顺序前沿 →
/// 收尾回绕。`is_cancelled` 在每个分块边界检查（条目被逐出 = 用户切走，
/// 立即收工让出串行队列）。`cap` 是有界填充（预取）的动态上限：
/// `Some(limit)` 时顺序填充到该字节数即收工，读者需求不受限；`None` 填满
/// 整集。逐决策点求值，预取条目升格为当前集（或已有读者在等）的瞬间自动解除。
async fn fill_remaining(
    sparse: &Arc<crate::domain::mp4::streaming::SparseBuffer>,
    prog: &Arc<ProgressiveStream>,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
    reporter: &ProgressReporter,
    is_cancelled: &(dyn Fn() -> bool + Send + Sync),
    cap: &(dyn Fn() -> Option<u64> + Send + Sync),
) -> AppResult<()> {
    let client = crate::domain::api::client::build_client(&settings.proxy)?;
    fill_remaining_with_client(
        &client,
        sparse,
        prog,
        &play.url,
        reporter,
        is_cancelled,
        cap,
    )
    .await
}

/// 调度器一次决策的结果。纯逻辑、无 I/O——回绕的终止性质在单测里钉死，
/// 填充循环只负责执行。
enum Decision {
    /// 下这一段 `[start, end)`
    Fetch(u64, u64),
    /// 决策点之后全满、洞在决策点之前：回绕到 0（只会发生一次，见 [`next_target`]）
    Wrap,
    /// 整集已覆盖，收工
    Done,
}

/// 从 `frontier` 找下一个要下的洞；前沿之后全满但整集未满 → 回绕。
///
/// 回绕在下一轮 `next_gap(0)` 必能找到洞（否则整集已满、走 `Done` 分支），
/// 所以 Wrap 之后紧跟一次实际下载，**不存在连续 Wrap 的路径**——旧版
/// 「回绕补洞」被 seek_hint 顶回原地空转的死循环，从这里结构上排除。
fn next_target(sparse: &crate::domain::mp4::streaming::SparseBuffer, frontier: u64) -> Decision {
    match sparse.next_gap(frontier) {
        Some((start, end)) => Decision::Fetch(start, end),
        None if sparse.downloaded() < sparse.len() => Decision::Wrap,
        None => Decision::Done,
    }
}

/// 三级填充调度器：
///
/// 1. **需求优先**：任一等待中的读者（[`crate::protocol::stream::ReaderDemand`])
///    的区间先填，按登记到达顺序（FIFO，多读者不互相饿死）；**回跳的 seek
///    与前进的一视同仁**——旧版单调 `seek_hint` 降不下来、回绕又被它顶住
///    的死循环，在机制上不再可能。
/// 2. **顺序续填**：没有需求时从填充前沿向文件尾顺序填——正是播放的自然
///    方向。前沿被需求跳走后留下的洞由第 3 级兜底。
/// 3. **收尾回绕**：前沿到尾之后若还有洞（跳填留下的），从 0 扫一遍补齐。
///    此时不可能有任何需求覆盖它——需求的优先级更高、且会即时打断回绕，
///    所以回绕不会再被任何提示顶住。
///
/// 每个决策点要么发起一次下载（await）、要么返回；短读按实际字节数推进
/// 前沿，不跳过缺口。不存在空转路径。
async fn fill_remaining_with_client(
    client: &reqwest::Client,
    sparse: &Arc<crate::domain::mp4::streaming::SparseBuffer>,
    prog: &Arc<ProgressiveStream>,
    cdn_url: &str,
    reporter: &ProgressReporter,
    is_cancelled: &(dyn Fn() -> bool + Send + Sync),
    cap: &(dyn Fn() -> Option<u64> + Send + Sync),
) -> AppResult<()> {
    use crate::domain::api::client::get_video_range;
    let total = sparse.len();
    let mut frontier = 0u64; // 顺序填充的前沿（密文偏移）
    loop {
        if is_cancelled() {
            log::info!(
                "[Online][probe] 填充中止: downloaded={}/{} frontier={frontier}",
                sparse.downloaded(),
                total
            );
            return Ok(());
        }

        // 决策 1：需求优先。跳到最早等待读者的第一个洞，前沿随之跟随
        //（服务完这个洞之后顺序续填就从这里继续——正是播放所在的位置）。
        let wanted = prog.demand.first_wanted(sparse);
        if let Some(want) = wanted {
            if want != frontier {
                log::info!(
                    "[Online] 读者需求优先：{want} 起有洞（顺序前沿在 {frontier}），跳转填充"
                );
            }
            frontier = want;
        } else if let Some(limit) = cap() {
            // 决策 2 的有界形态（预取）：没有读者在等也不是当前集，
            // 顺序填到上限就收工，串行队列让给真正在看的那一路。
            if sparse.downloaded() >= limit.min(total) {
                log::info!("[Online] 预填达到上限（{limit} 字节），余下等转正续填");
                return Ok(());
            }
        }

        // 决策 2/3：顺序前沿找洞，前沿之后全满则回绕。
        let (gap_start, gap_end) = match next_target(sparse, frontier) {
            Decision::Fetch(start, end) => (start, end),
            Decision::Wrap => {
                log::info!(
                    "[Online][probe] 收尾回绕：前沿之后已满（downloaded={}/{}），从 0 补剩余的洞",
                    sparse.downloaded(),
                    total
                );
                frontier = 0;
                continue;
            }
            Decision::Done => {
                log::info!("[Online][probe] 填充自然完成 downloaded={total}");
                return Ok(());
            }
        };
        let fetch_end = gap_start + FETCH_CHUNK.min(gap_end - gap_start);
        // 探针（排查填充停摆用）：区间请求的始末都留痕，卡在哪一段一目了然
        let probe_at = std::time::Instant::now();
        log::debug!("[Online][probe] 区间 {gap_start}-{fetch_end} 请求开始");
        let bytes = match get_video_range(client, cdn_url, gap_start, fetch_end - 1).await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[Online][probe] 区间 {gap_start}-{fetch_end} 失败({e:?})");
                return Err(e);
            }
        };
        log::info!(
            "[Online][probe] 区间 {gap_start}-{fetch_end} 返回 {}B 耗时{}ms",
            bytes.len(),
            probe_at.elapsed().as_millis()
        );
        let want = fetch_end - gap_start;
        let got = bytes.len() as u64;
        if got == 0 {
            // 空响应意味着 write 不会标记任何覆盖，同一决策点会反复选中
            // 这里——静默空转。直接报错让上层看见。
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
        log::debug!(
            "[Online] 填充区间 {gap_start}-{} 完成（累计 {done}/{total}）",
            gap_start + got
        );
        reporter.report(done, total, "downloading");
        // 短读也按实际字节数推进前沿：缺口没填完就不越过它，下一轮从
        // 剩余处继续（旧版直接跳到 fetch_end，缺口被甩在前沿后面，
        // 只能指望回绕兜底——回绕又被 seek_hint 顶死，正是停摆根因）。
        frontier = gap_start + got;
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
    /// 前端按 `{seriesId}:{vidIndex}` 过滤事件（episodeKey，与
    /// compat-play-progress 同一格式）——曾经发裸 vid 导致「正在缓存 X%」
    /// 永远匹配不上，别改回 vid。
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

#[cfg(test)]
mod scheduler_tests {
    use super::*;

    /// 回绕是一次决策而不是循环：Wrap 之后从 0 必能取到洞，全满则 Done。
    /// 旧版死循环（回绕 → hint 顶回原地 → 再回绕）在这里不可能复现：
    /// 决策函数没有任何可以「顶回」游标的外部状态。
    #[test]
    fn wrap_is_a_decision_not_a_loop() {
        let sparse = crate::domain::mp4::streaming::SparseBuffer::new(100);
        sparse.write(50, &[0u8; 5]); // 50-55
        sparse.write(90, &[0u8; 10]); // 90-100
                                      // 前沿在 90：其后全满、整集未满 → 回绕
        assert!(matches!(next_target(&sparse, 90), Decision::Wrap));
        // 回绕到 0 后：第一个洞从 0 起
        assert!(matches!(next_target(&sparse, 0), Decision::Fetch(0, 50)));
        // 模拟把 0-50 填上：下一个洞是 55-90，无需再回绕
        sparse.write(0, &[0u8; 50]);
        assert!(matches!(next_target(&sparse, 0), Decision::Fetch(55, 90)));
        // 填满后任意前沿都直接 Done
        sparse.write(55, &[0u8; 35]);
        assert!(matches!(next_target(&sparse, 0), Decision::Done));
        assert!(matches!(next_target(&sparse, 99), Decision::Done));
    }

    /// 需求登记（ReaderDemand）把洞指到前沿**之前**时，调度器跟着跳回去：
    /// 这正是旧版单调 seek_hint 做不到的「回跳」。
    #[test]
    fn demand_points_backwards_into_early_hole() {
        use crate::protocol::stream::ReaderDemand;

        let sparse = crate::domain::mp4::streaming::SparseBuffer::new(100);
        sparse.write(90, &[0u8; 10]); // 尾部已就绪（seek 到片尾之类的场景）
        let d = ReaderDemand::new();
        d.register(&[(10, 20)]); // 读者在等前沿之前的洞（回跳 seek）

        let want = d.first_wanted(&sparse).expect("应定位到读者的洞");
        assert_eq!(want, 10);
        // 调度器把前沿对齐到需求：从 10 起的洞就是下一个目标，
        // 而不是被任何高位状态顶回 90
        assert!(matches!(
            next_target(&sparse, want),
            Decision::Fetch(10, 90)
        ));
    }
}
