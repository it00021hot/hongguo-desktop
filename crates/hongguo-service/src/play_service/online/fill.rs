//! 填充门闸与填充循环：FILL_GATE 串行门闸、渐进/整集两条填充路径、
//! 三级调度器（需求优先 → 顺序前沿 → 收尾回绕）与预取动态预算。

use std::sync::Arc;
use std::time::Duration;

use hongguo_api::client::{STREAM_RETRY_ATTEMPTS, stream_retry_backoff};
use hongguo_common::model::Settings;
use hongguo_common::{AppError, AppResult};
use hongguo_stream::{ProgressiveStream, StreamCache, cache};

use super::plain::fetch_plain_with;
use super::progress::{ProgressReporter, event_sink};
use crate::app_state::AppState;

/// 填充门闸：数据填充段（真正抢带宽的下载+解密）同时只允许一路。
///
/// 渐进注册（探针链，~1MB 流量）**不排门闸**——它越早完成，切剧能命中
/// 缓存的面越大；连它一起串行的话，快速滚动时下一部的注册会被当前集的
/// 整集填充压住，切过去就得现场重走整条探针链（滚轮后 1-2s 的主因）。
/// 拿着门闸的调用方在 `begin_fetch` 已登记取流权，不会重复发请求。
static FILL_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// 下载一集、解密、整段推进缓存。
///
/// 先尝试渐进路径（首帧不等整集）；计划构建失败才回落整集路径，
/// 回落后行为与旧版完全一致。注册成功后的填充阶段失败则直接上抛——
/// 那时回落只会把整集再重下一遍，和今天的失败语义一样交给上层清理。
///
/// **数据填充段全局串行**（[`FILL_GATE`]）：渐进注册不排队（见 [`FILL_GATE`]），
/// 拿到门闸后才进入真正抢带宽的填充段，一次只填一路。并发两路整集下载+解密
/// 会抢爆带宽与 CPU——用户实测卡死的那次，当前剧与预取同时 fill 是最可疑的现场。
///
/// **可取消**（2026-10-06）：条目被逐出（用户切走）后填充立即中止让出队列——
/// 否则被切走的前一集拖着整集下载占住串行通道，当前集黑屏干等到天荒地老。
/// 每个分块边界都检查一次，最坏滞后一个分块（1MB）。
///
/// 填充预算只有一条规则：**正在播的那一集缓存整集，其余只缓存开头
/// [`NON_CURRENT_HEAD_PERCENT`]%**（见 [`fill_budget`]）。预算是动态的：
/// 条目升格为当前集、或已有读者在等这一档，即刻转无界续满；当前集
/// （另一条流）的读者在等数据则立刻让位收工（见 [`FillBudget`]）——
/// 不必依赖 [`resume_fill`] 事后抢取流权（预取尚未收工时它抢不到，
/// 那正是「播到头部上限就断流」的窗口）。
///
/// `prefetch` 只影响**让位判定**：预填被切走（条目还在）就停下，当前集的
/// 填充被切走则立即中止（见 `cancelled`）。
#[expect(
    clippy::too_many_arguments,
    reason = "参数表与平台 C API（虚表/会话属性）一一对应，硬拆参数结构反而失真"
)]
pub(super) async fn fill(
    state: &AppState,
    c: &StreamCache,
    vid: &str,
    progress_key: &str,
    definition: u32,
    play: &hongguo_api::play_url::PlayInfo,
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
    if cancelled() {
        log::info!("[Online] {vid} 已被切走/让位，取消填充");
        return Ok(());
    }
    let reporter = ProgressReporter::new(event_sink(state.events()), progress_key.to_string());

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
            // 注册完成后才排门闸：条目已可应答（`<video>` 能取到已就绪的
            // 窗口），门闸只串行化接下来的大流量填充段。
            let _gate = FILL_GATE.acquire().await;
            if cancelled() {
                log::info!("[Online] {vid} 排到门闸时已切走，中止填充");
                return Ok(());
            }
            // CDN 抖动（短读/断流）是常态而不是异常，整体重试而不是一次失败
            // 就永远停在前沿——播放追上未填充区只能超时报错。
            // 预算闭包逐决策点求值：正在播的那一集无界，其余只填开头一段
            let cap: Box<dyn Fn() -> FillBudget + Send + Sync> = {
                let key = vid.to_string();
                let prog_cap = prog.clone();
                Box::new(move || fill_budget(cache(), &key, &prog_cap))
            };
            let mut last_err = None;
            for attempt in 0..STREAM_RETRY_ATTEMPTS {
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
                        // 退避与 client 的 Range 重试同一份 it() 口径
                        tokio::time::sleep(stream_retry_backoff(attempt)).await;
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
            // 整集下载是流量最大的一档，进门闸串行
            let _gate = FILL_GATE.acquire().await;
            if cancelled() {
                log::info!("[Online] {vid} 排到门闸时已切走，整集回落取消");
                return Ok(());
            }
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

/// 非当前集只缓存开头这么多百分比。
///
/// 缓存规则只有一条：**正在播的那一集缓存整集，其余一律只缓存开头
/// [`NON_CURRENT_HEAD_PERCENT`]%**——下一集、连播预取的后续集、换剧预取的
/// 后续部都算「其余」。转正（`prepare` → `resume_fill`）后按整集续满。
///
/// 为什么是比例而不是固定字节：短剧单集只有 6-10MB（长的 30-80MB），固定
/// 头部在小集上占比过大、在大集上又不够；比例口径对长短集一致。
const NON_CURRENT_HEAD_PERCENT: u64 = 20;

/// 非当前集的头部上限（字节）：整集的 [`NON_CURRENT_HEAD_PERCENT`]%。
fn head_limit(prog: &ProgressiveStream) -> u64 {
    prog.sparse.len().saturating_mul(NON_CURRENT_HEAD_PERCENT) / 100
}

/// 填充预算决策（逐决策点求值）。
///
/// - 正在播的那一集 → 无上限，缓存整集：当前集（`set_current` 记），或本流
///   已有读者在等（读者在等就说明它已经在播）；
/// - 当前集（**另一条**流）的读者正在等数据 → 立刻让位收工，带宽让给在播
///   的那一路；
/// - 其余 → 只填开头 [`NON_CURRENT_HEAD_PERCENT`]%，余下等转正续填。
fn fill_budget(c: &StreamCache, vid: &str, prog: &ProgressiveStream) -> FillBudget {
    if c.current() == vid || prog.demand.has_waiters() {
        return FillBudget::Unbounded;
    }
    if c.current_has_waiters() {
        return FillBudget::Yield;
    }
    FillBudget::Bounded(head_limit(prog))
}

/// 填充调度器一次容量决策的结果（有界填充逐决策点求值）。
///
/// - [`FillBudget::Unbounded`]：不限量填满整集——已升格为当前集，或本流
///   自己的读者在等（需求由同一条填充按「需求优先」即时供给）；
/// - [`FillBudget::Bounded`]：顺序填到该字节数收工，余下等转正续填；
/// - [`FillBudget::Yield`]：立刻收工让出门闸——**另一条流**（当前集）的
///   读者正在等数据，带宽该让给在播的那一路。条目保留，转正后由
///   [`resume_fill`] 续满。
enum FillBudget {
    Unbounded,
    Bounded(u64),
    Yield,
}

/// 续填在「预取填充还握着取流权」时的退避重试：预取正好在头部预算处收尾，
/// 这一次 begin_fetch 会失败；不重试就会留下「头部之后永远没人续填」的缝。
const RESUME_RETRY_ATTEMPTS: u8 = 8;
const RESUME_RETRY_DELAY: Duration = Duration::from_millis(200);

/// 条目转正为当前集后的**续填**：渐进已注册、URL/代理存在条目里，
/// 直接把余下字节按序填满（当前集优先级，不礼让）。
///
/// 没有它，只填了头部的条目在被切上后，播放追上填充前沿就只能
/// 30s 超时——「滚回来就黑屏」的另一半。
pub fn resume_fill(state: &AppState, vid: &str, definition: u32, progress_key: &str) {
    resume_fill_inner(state, vid, definition, progress_key, RESUME_RETRY_ATTEMPTS);
}

fn resume_fill_inner(
    state: &AppState,
    vid: &str,
    definition: u32,
    progress_key: &str,
    attempts_left: u8,
) {
    let c = cache();
    if !c.needs_resume(vid, definition) {
        return;
    }
    if !c.begin_fetch(vid, definition) {
        // 已有填充方（多半是 8MB 收尾中的预取）。退避后再试，别让 loader
        // 在填充前沿干等 30s 超时。
        if attempts_left == 0 {
            log::warn!("[Online] {vid}@{definition} 续填一直抢不到取流权（退避重试耗尽）");
            return;
        }
        let state = state.clone();
        let owned = vid.to_string();
        let owned_key = progress_key.to_string();
        crate::runtime::spawn(async move {
            tokio::time::sleep(RESUME_RETRY_DELAY).await;
            resume_fill_inner(&state, &owned, definition, &owned_key, attempts_left - 1);
        });
        return;
    }
    let Some((sparse, prog)) = c.resume_parts(vid, definition) else {
        c.end_fetch(vid, definition);
        return;
    };
    let proxy = prog.proxy.clone();
    let cdn_url = prog.cdn_url.clone();
    let state = state.clone();
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    // 本函数是同步入口，可能被协议/回调线程调用：必须用共用任务运行时，
    // 见模块头注释。
    crate::runtime::spawn(async move {
        let reporter = ProgressReporter::new(event_sink(state.events()), owned_key);
        let cancelled = || {
            !c.exists(&owned, definition)
                || (c.current() != owned && !c.is_next(&owned, definition))
        };
        let client = match hongguo_api::client::build_client(&proxy) {
            Ok(cl) => cl,
            Err(e) => {
                log::warn!("[Online] 续填 {owned} 建 client 失败: {e}");
                c.end_fetch(&owned, definition);
                return;
            }
        };
        // 预算与预填同一套规则：转正后（当前集）无上限填满；尚未转正的
        // （系统播放器队列里的下一集提前续填）仍只填开头一段。
        let cap: Box<dyn Fn() -> FillBudget + Send + Sync> = {
            let key = owned.clone();
            let prog_cap = prog.clone();
            Box::new(move || fill_budget(cache(), &key, &prog_cap))
        };
        let result = fill_remaining_with_client(
            &client, &sparse, &prog, &cdn_url, &reporter, &cancelled, &cap,
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
/// 探针序列（实证口径，见 captures/ 下的前端脚本）：
/// 首探 256KB → 首探里没 moov 就从「最后一个顶层盒的结束处」64KB 步进游走
/// （最多 4 轮）→ 精确区间拉 moov 本体。**不做固定尾部预取**——多下的字节
/// 在 moov 拉完前纯属浪费首帧时间；尾部数据的即时性由读者需求登记保证。
///
/// 返回 `Err` 的所有情形都应回落整集路径（CDN 不支持 Range、找不到 moov、
/// 样本表解析失败……）。这一阶段完成时协议层已可应答，错误只会发生在
/// 「还没有任何对外可见状态」的时候，回落没有半成品要清理。
async fn build_progressive(
    play: &hongguo_api::play_url::PlayInfo,
    settings: &Settings,
    report: &(dyn Fn(u64, u64) + Sync),
) -> AppResult<(
    Arc<hongguo_video::streaming::SparseBuffer>,
    Arc<ProgressiveStream>,
)> {
    use hongguo_api::client::{get_video_range, probe_video_len};
    use hongguo_video::streaming::{SparseBuffer, StreamingPlan, locate_moov, top_boxes_end};

    let client = hongguo_api::client::build_client(&settings.proxy)?;
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
        let key = hongguo_video::crypto::key_derive::derive_key(&play.key_material)?;
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
/// 立即收工让出串行队列）。`cap` 是预取等有界填充的容量决策：[`FillBudget`]
/// 逐决策点求值，预取条目升格为当前集、读者在等、或当前集开始等数据的
/// 瞬间自动切换形态。
async fn fill_remaining(
    sparse: &Arc<hongguo_video::streaming::SparseBuffer>,
    prog: &Arc<ProgressiveStream>,
    play: &hongguo_api::play_url::PlayInfo,
    settings: &Settings,
    reporter: &ProgressReporter,
    is_cancelled: &(dyn Fn() -> bool + Send + Sync),
    cap: &(dyn Fn() -> FillBudget + Send + Sync),
) -> AppResult<()> {
    let client = hongguo_api::client::build_client(&settings.proxy)?;
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
fn next_target(sparse: &hongguo_video::streaming::SparseBuffer, frontier: u64) -> Decision {
    match sparse.next_gap(frontier) {
        Some((start, end)) => Decision::Fetch(start, end),
        None if sparse.downloaded() < sparse.len() => Decision::Wrap,
        None => Decision::Done,
    }
}

/// 三级填充调度器：
///
/// 1. **需求优先**：任一等待中的读者（[`hongguo_stream::ReaderDemand`])
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
    sparse: &Arc<hongguo_video::streaming::SparseBuffer>,
    prog: &Arc<ProgressiveStream>,
    cdn_url: &str,
    reporter: &ProgressReporter,
    is_cancelled: &(dyn Fn() -> bool + Send + Sync),
    cap: &(dyn Fn() -> FillBudget + Send + Sync),
) -> AppResult<()> {
    use hongguo_api::client::get_video_range;
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
        } else {
            // 决策 2 的容量形态（预取）：没有读者在等时按预算走——逐决策点
            // 求值，升格为当前集或当前集开始等数据的瞬间自动切换。
            match cap() {
                FillBudget::Yield => {
                    log::info!("[Online] 当前集读者在等数据，预取让位收工，余下等转正续填");
                    return Ok(());
                }
                FillBudget::Bounded(limit) => {
                    // 按**顺序填充前沿**判，不按已下载总量：头部探测与 moov
                    // 也计入已下载，短集（6-10MB）上它们就能把 20% 的预算吃光，
                    // 头部反而没填到。前沿才是「开头填到了哪儿」的真值。
                    if frontier >= limit.min(total) {
                        log::info!(
                            "[Online] 预填达到上限（顺序填到 {limit}/{total} 字节），余下等转正续填"
                        );
                        return Ok(());
                    }
                }
                FillBudget::Unbounded => {}
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
    sparse: &hongguo_video::streaming::SparseBuffer,
    start: u64,
    end: u64,
) -> Vec<u8> {
    let mut out = vec![0u8; (end - start) as usize];
    sparse.read_into(start, &mut out);
    out
}

/// 头部首探量：ftyp + mdat 头都装得下，顶层盒表从这里算出 moov 落点。
const HEAD_BYTES: u64 = 256 * 1024;
/// moov 不在首探里时的盒游走步进：从「最后一个顶层盒的结束处」起探。
const BOX_WALK_STEP: u64 = 64 * 1024;
/// 盒游走最多轮数：正常尾-moov 布局第一轮就命中，多轮只防非常规盒序
/// （fragmented 之类）。
const BOX_WALK_MAX: usize = 4;
/// 顺序填充的步长：每段一个 Range 请求。
///
/// 1MB，实测值——分块按 mp4 样本表分组拉流，单请求上限 `1<<20`
/// （首探 256KB、盒游走 64KB 步进）。
/// 分块越小，seek 需求打断顺序填充的粒度越细，回跳响应越快。
const FETCH_CHUNK: u64 = 1024 * 1024;

#[cfg(test)]
mod scheduler_tests {
    use super::*;

    /// 造一条指定长度的渐进流（不加密、不注册需求）。
    fn progressive_of(total: u64) -> Arc<ProgressiveStream> {
        Arc::new(ProgressiveStream {
            sparse: hongguo_video::streaming::SparseBuffer::new(total),
            plan: None,
            plain_len: total,
            demand: Default::default(),
            cdn_url: String::new(),
            proxy: hongguo_common::model::ProxyConfig::default(),
        })
    }

    /// 缓存规则钉死：**正在播的那一集缓存整集，其余只缓存开头 20%**。
    #[test]
    fn only_the_playing_episode_is_cached_whole() {
        const TOTAL: u64 = 10_000_000;
        let c = StreamCache::default();
        let prog = progressive_of(TOTAL);

        // 其余的（预取的下一集等）：只留开头 20%
        assert!(
            matches!(
                fill_budget(&c, "v-prefetch", &prog),
                FillBudget::Bounded(2_000_000)
            ),
            "非当前集应只填开头 20%"
        );

        // 正在播的那一集：整集，无上限
        c.set_current("v-playing");
        assert!(matches!(
            fill_budget(&c, "v-playing", &prog),
            FillBudget::Unbounded
        ));

        // 已经有读者在等这一档 = 它已经在播：同样无上限
        prog.demand.register(&[(0, 10)]);
        assert!(matches!(
            fill_budget(&c, "v-watched", &prog),
            FillBudget::Unbounded
        ));
    }

    /// 当前集（另一条流）的读者在等数据时，其余的流立刻让位收工。
    #[test]
    fn non_current_yields_to_the_playing_episode() {
        let c = StreamCache::default();
        c.set_current("v-current");
        let current = progressive_of(1_000);
        c.begin_fetch("v-current", 1080);
        c.set_progressive("v-current", 1080, current.clone());
        current.demand.register(&[(0, 10)]);

        let other = progressive_of(10_000_000);
        assert!(
            matches!(fill_budget(&c, "v-other", &other), FillBudget::Yield),
            "在播的那一路在等数据时，其余的流该让位"
        );
    }

    /// 回绕是一次决策而不是循环：Wrap 之后从 0 必能取到洞，全满则 Done。
    /// 旧版死循环（回绕 → hint 顶回原地 → 再回绕）在这里不可能复现：
    /// 决策函数没有任何可以「顶回」游标的外部状态。
    #[test]
    fn wrap_is_a_decision_not_a_loop() {
        let sparse = hongguo_video::streaming::SparseBuffer::new(100);
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
        use hongguo_stream::ReaderDemand;

        let sparse = hongguo_video::streaming::SparseBuffer::new(100);
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
