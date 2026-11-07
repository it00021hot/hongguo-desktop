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

/// 填充门闸：数据填充段（真正抢带宽的下载+解密）同时只允许一路。渐进注册（探针链）**不排门闸**——
/// 它越早完成，切剧能命中缓存的面越大；拿着门闸的调用方在 `begin_fetch` 已登记取流权，不会重复发请求。
static FILL_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// 下载一集、解密、整段推进缓存：先渐进（首帧不等整集），计划构建失败回落整集；数据填充段经
/// [`FILL_GATE`] 全局串行，条目被逐出立即中止让位；预算=当前集整集、其余只填头部 [`NON_CURRENT_HEAD_PERCENT`]%。
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
    // 让位规则：用户滚走后条目虽被保留（弹回来秒切），但它的下载必须立即让位——
    // 串行队列只有一路，被切走剧的整集下载拖住队头，当前剧就只能干等。预取（有界头部）除外。
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
            // 注册完成后才排门闸：条目已可应答，门闸只串行化接下来真正抢带宽的填充段。
            let _gate = FILL_GATE.acquire().await;
            if cancelled() {
                log::info!("[Online] {vid} 排到门闸时已切走，中止填充");
                return Ok(());
            }
            // CDN 抖动（短读/断流）是常态，整体重试而不是一次失败就停在前沿。
            // 预算闭包逐决策点求值：正在播的那一集无界，其余只填开头一段。
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
            if crate::device::note_plain_video_sample(state, &plain) {
                log::info!("[Online] {vid} 明文里有视频样本");
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

/// 非当前集只缓存开头这么多百分比：下一集、连播预取的后续集、换剧预取的后续部都算「其余」，转正后按整集续满。
/// 用比例而非固定字节：短剧单集 6-10MB（长的 30-80MB），比例口径对长短集一致。
const NON_CURRENT_HEAD_PERCENT: u64 = 20;

/// 非当前集的头部上限（字节）：整集的 [`NON_CURRENT_HEAD_PERCENT`]%。
fn head_limit(prog: &ProgressiveStream) -> u64 {
    prog.sparse.len().saturating_mul(NON_CURRENT_HEAD_PERCENT) / 100
}

/// 填充预算决策（逐决策点求值）：正在播的那一集（或本流已有读者在等）→ 无上限整集；
/// 当前集（**另一条**流）的读者在等数据 → 立刻让位收工；其余 → 只填开头 [`NON_CURRENT_HEAD_PERCENT`]%。
fn fill_budget(c: &StreamCache, vid: &str, prog: &ProgressiveStream) -> FillBudget {
    if c.current() == vid || prog.demand.has_waiters() {
        return FillBudget::Unbounded;
    }
    if c.current_has_waiters() {
        return FillBudget::Yield;
    }
    FillBudget::Bounded(head_limit(prog))
}

/// 填充调度器一次容量决策的结果：`Unbounded` 填满整集（已升格当前集或本流读者在等）、
/// `Bounded` 顺序填到该字节数收工、`Yield` 立刻收工让出门闸（条目保留，转正后由 `resume_fill` 续满）。
enum FillBudget {
    Unbounded,
    Bounded(u64),
    Yield,
}

/// 续填在「预取填充还握着取流权」时的退避重试：预取正好在头部预算处收尾，
/// 这一次 begin_fetch 会失败；不重试就会留下「头部之后永远没人续填」的缝。
const RESUME_RETRY_ATTEMPTS: u8 = 8;
const RESUME_RETRY_DELAY: Duration = Duration::from_millis(200);

/// 条目转正为当前集后的**续填**：渐进已注册、URL/代理存在条目里，直接把余下字节按序填满（当前集优先级，不礼让）。
/// 没有它，只填了头部的条目在被切上后，播放追上填充前沿就只能超时——「滚回来就黑屏」的另一半。
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
        // 已有填充方（多半是收尾中的预取）：退避后再试，别让 loader 在填充前沿干等超时。
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
    // 同步入口，可能被协议/回调线程调用：必须用共用任务运行时（红线）。
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
        // 预算与预填同一套规则：转正后（当前集）无上限填满；尚未转正的（下一集提前续填）仍只填头部。
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

/// 渐进路径第一阶段：首探头部 256KB、盒游走定位 moov（64KB 步进、最多 4 轮）、精确补齐、建计划、注册协议层。
/// 不做固定尾部预取——多下的字节在 moov 拉完前纯属浪费首帧时间；`Err` 一律回落整集路径，此阶段错误无半成品要清理。
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
            sparse.write(from, &seg);
            report(sparse.downloaded(), total);
            if let Some(found) = locate_moov(&seg, from, total) {
                located = Some(found);
                break;
            }
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
        // moov 密文已折进计划；稀疏里再留一份是双份（连播时每集数 MB 的「rss 残渣」主因）。
        // covered 保留，填充不会回填；渲染走计划里的重建 moov，不再读这段密文。
        sparse.forget_data(moov_start, moov_start + moov_size);
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

/// 渐进路径第二阶段：把余下字节填满，期间响应读者需求。`is_cancelled` 在每个分块边界检查
///（条目被逐出 = 用户切走，立即收工让出串行队列）；`cap` 逐决策点求值，形态可自动切换。
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

enum Decision {
    /// 下这一段 `[start, end)`
    Fetch(u64, u64),
    /// 决策点之后全满、洞在决策点之前：回绕到 0（只会发生一次，见 `next_target`）
    Wrap,
    /// 整集已覆盖，收工
    Done,
}

/// 从 `frontier` 找下一个要下的洞；前沿之后全满但整集未满 → 回绕。
/// 回绕后下一轮 `next_gap(0)` 必能找到洞（否则整集已满走 `Done`），不存在连续 Wrap 的路径。
fn next_target(sparse: &hongguo_video::streaming::SparseBuffer, frontier: u64) -> Decision {
    match sparse.next_gap(frontier) {
        Some((start, end)) => Decision::Fetch(start, end),
        None if sparse.downloaded() < sparse.len() => Decision::Wrap,
        None => Decision::Done,
    }
}

/// 三级填充调度器：需求优先（等待读者的区间先填，FIFO、回跳与前进一视同仁）→ 顺序续填（前沿向文件尾）→
/// 收尾回绕（前沿到尾后从 0 补齐跳填留下的洞）。每个决策点要么下载要么返回；
/// 短读按实际字节数推进前沿，不跳过缺口，不存在空转路径。
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

        // 决策 1：需求优先——跳到最早等待读者的第一个洞，前沿随之跟随
        let wanted = prog.demand.first_wanted(sparse);
        if let Some(want) = wanted {
            if want != frontier {
                log::info!(
                    "[Online] 读者需求优先：{want} 起有洞（顺序前沿在 {frontier}），跳转填充"
                );
            }
            frontier = want;
        } else {
            // 决策 2 的容量形态（预取）：没有读者在等时按预算走，逐决策点求值。
            match cap() {
                FillBudget::Yield => {
                    log::info!("[Online] 当前集读者在等数据，预取让位收工，余下等转正续填");
                    return Ok(());
                }
                FillBudget::Bounded(limit) => {
                    // 按顺序填充前沿判，不按已下载总量：头部探测与 moov 也计入已下载，
                    // 短集上它们就能把预算吃光；前沿才是「开头填到了哪儿」的真值。
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
            // 空响应不会标记任何覆盖，同一决策点会反复空转——直接报错让上层看见。
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
        // 短读也按实际字节数推进前沿：缺口没填完就不越过它，下一轮从剩余处继续。
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
/// 顺序填充的步长（每段一个 Range 请求）：1MB 分块按 mp4 样本表分组拉流；
/// 分块越小，seek 需求打断顺序填充的粒度越细，回跳响应越快。
const FETCH_CHUNK: u64 = 1024 * 1024;

#[cfg(test)]
mod scheduler_tests {
    use super::*;

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

    #[test]
    fn only_the_playing_episode_is_cached_whole() {
        const TOTAL: u64 = 10_000_000;
        let c = StreamCache::default();
        let prog = progressive_of(TOTAL);

        assert!(
            matches!(
                fill_budget(&c, "v-prefetch", &prog),
                FillBudget::Bounded(2_000_000)
            ),
            "非当前集应只填开头 20%"
        );

        c.set_current("v-playing");
        assert!(matches!(
            fill_budget(&c, "v-playing", &prog),
            FillBudget::Unbounded
        ));

        prog.demand.register(&[(0, 10)]);
        assert!(matches!(
            fill_budget(&c, "v-watched", &prog),
            FillBudget::Unbounded
        ));
    }

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

    #[test]
    fn wrap_is_a_decision_not_a_loop() {
        let sparse = hongguo_video::streaming::SparseBuffer::new(100);
        sparse.write(50, &[0u8; 5]);
        sparse.write(90, &[0u8; 10]);
        assert!(matches!(next_target(&sparse, 90), Decision::Wrap));
        assert!(matches!(next_target(&sparse, 0), Decision::Fetch(0, 50)));
        sparse.write(0, &[0u8; 50]);
        assert!(matches!(next_target(&sparse, 0), Decision::Fetch(55, 90)));
        sparse.write(55, &[0u8; 35]);
        assert!(matches!(next_target(&sparse, 0), Decision::Done));
        assert!(matches!(next_target(&sparse, 99), Decision::Done));
    }

    #[test]
    fn demand_points_backwards_into_early_hole() {
        use hongguo_stream::ReaderDemand;

        let sparse = hongguo_video::streaming::SparseBuffer::new(100);
        sparse.write(90, &[0u8; 10]);
        let d = ReaderDemand::new();
        d.register(&[(10, 20)]);

        let want = d.first_wanted(&sparse).expect("应定位到读者的洞");
        assert_eq!(want, 10);
        assert!(matches!(
            next_target(&sparse, want),
            Decision::Fetch(10, 90)
        ));
    }
}
