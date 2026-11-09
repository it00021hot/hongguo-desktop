//! 取流准备与预取编排：解析档位、抢取流权、复用/等待在途取流、连播预取。

use std::collections::HashMap;
use std::sync::LazyLock;

use hongguo_common::model::VideoDefinition;
use hongguo_common::{AppError, AppResult};
use hongguo_stream::{StreamCache, cache};
use parking_lot::Mutex;

use super::fill::{fill, resume_fill};
use crate::app_state::AppState;

/// `(vid, definition) → codec_type`：缓存命中时仍要知道要不要走兼容转码。
static STREAM_CODECS: LazyLock<Mutex<HashMap<(String, u32), String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn remember_codec(vid: &str, definition: u32, codec: &str) {
    STREAM_CODECS
        .lock()
        .insert((vid.to_string(), definition), codec.to_string());
}

fn remembered_codec(vid: &str, definition: u32) -> String {
    STREAM_CODECS
        .lock()
        .get(&(vid.to_string(), definition))
        .cloned()
        .unwrap_or_default()
}

/// 预取（换剧 + 连播共用）的并发上限：每路要先走「取流表 + 探针链」再排填充门闸，
/// 多路同时探针会在慢网/弱机上抢当前播放的带宽与 CPU；2 路足够让下一个目标先热。
const PREFETCH_CONCURRENCY: usize = 2;
static PREFETCH_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(PREFETCH_CONCURRENCY);

/// 取流表获取（类型注册表守门）：vid 的条目类型未注册时**不猜 key**——先拉本剧分集档案
///（解析侧会把整剧各集的 vid→content_type 一次注册齐）再取流。取流未命中（[`AppError::StreamMiss`]）
/// 如实上抛：服务端不发货是设备侧信任问题，应用层没有兜底解。
/// `wait_gate=false`：调用方已持有冷启动门闩（换档后独占首包），禁止再 wait，否则自锁。
async fn fetch_with_registry(
    state: &AppState,
    vid: &str,
    progress_key: &str,
    definition: Option<u32>,
    cap: hongguo_api::stream_pick::DecoderCapability,
    wait_gate: bool,
) -> AppResult<hongguo_api::play_url::PlayInfo> {
    // 空响应换档预热窗口会 rearm 门闩；默认再等一次，避免起播/预取在半新身份上打空包。
    // 持闩路径（EmptyResponse 恢复）必须跳过，否则与 ReleaseOnDrop 同作用域自等 PENDING。
    if wait_gate {
        crate::cold_start::wait().await;
    }
    let env = state.api_env();
    match hongguo_api::play_url::fetch_play_url(vid, definition, cap, &env).await {
        Ok(p) => Ok(p),
        Err(e @ AppError::ContentTypeUnknown(_)) => {
            // progress_key 形如 "{series_id}:{vid_index}"
            let series_id = progress_key.split(':').next().unwrap_or("").trim();
            if series_id.is_empty() || !series_id.bytes().all(|b| b.is_ascii_digit()) {
                return Err(e);
            }
            hongguo_api::detail::fetch_episode_list(series_id, &env).await?;
            hongguo_api::play_url::fetch_play_url(vid, definition, cap, &env).await
        }
        Err(e) => Err(e),
    }
}

/// 在线播放就绪后带回的档位信息。
pub struct Prepared {
    pub url: String,
    /// 实际选中的档位。请求的档位平台没给时会回退，这里是回退后的真实值。
    pub definition: u32,
    /// 本集提供的全部档位
    pub definitions: Vec<VideoDefinition>,
    /// 选中流编码（`bytevc2` 时前端/壳走兼容转码）。
    pub codec: String,
}

/// 准备在线播放，返回流地址与档位信息。
///
/// 缓存按 (vid, 档位) 分条目（见 [`hongguo_stream`]），所以切清晰度
/// 既不用下载、也不会动到正在播的那一档：切回去直接命中内存里的旧数据。
pub async fn prepare(
    state: &AppState,
    vid: &str,
    progress_key: &str,
    definition: Option<u32>,
    // 同剧集窗：换档成功后必须在新身份上重跑批量，否则邻集空包。
    series_window: Option<(&str, u32)>,
) -> AppResult<Prepared> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    crate::cold_start::wait().await;
    // 播放策略轮询（行为画像对齐）：首次在线播放触发，进程内仅启动一次
    super::super::ensure_play_strategy_poller(state);
    let c = cache();
    // 记下当前集：逐出策略保它、预填礼让它、切走后旧填充据此自杀。
    // 一次只看一集的**语义下有界保留**：全清的话，信息流切剧目标一弹，
    // 正在被 `<video>` 读的那条流就被逐掉，播放追上缓冲前沿只能超时。
    // set_current 内已跑 retain_playing_set（连播接力同路，不依赖 prepare）。
    c.set_current(vid);

    // 不指定档位时先认上一轮解析到的档位（缓存键必须有具体档位）；连 auto 记录都没有时，
    // 认领同 vid 任一已就绪条目，避免整条取流链路重走一遍，也让「拉分集」与「起播」
    // 两个并发请求认到同一条缓存。
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
            resume_fill(state, vid, want, progress_key);
            log::info!("[Online] {vid} 档位 {want} 已缓存，直接复用");
            crate::device::note_play_address(state);
            return Ok(hit);
        }
        // 另一个请求正在把这一档取下来：等它，而不是自己也发一次下载。
        if c.is_fetching(vid, want) {
            log::info!("[Online] {vid} 档位 {want} 正在取流，等待复用");
            return wait_for_fetch(c, vid, want).await;
        }
    }

    // 还没有键可查：先做一次很小的接口调用把流表拿回来，解析出的真实档位才是缓存的键；
    // 贵的那一步（整集下载 + 解密）要等拿到取流权之后才发生。
    let cap = crate::device::stream_cap();
    let play = match fetch_with_registry(state, vid, progress_key, definition, cap, true).await {
        Err(e @ AppError::StreamMiss(_)) => {
            crate::device::note_stream_miss(state, vid);
            return Err(e);
        }
        // 全程空字节：现役身份被拒发货 → 匿名换档一次再取
        Err(AppError::EmptyResponse(msg)) => {
            log::warn!("[Online] {vid} 取流空响应：{msg}");
            if !crate::device::recover_after_empty_stream(state, vid).await {
                return Err(AppError::EmptyResponse(msg));
            }
            // recover 返回时冷启动门闩已释放；信息流预取/其它 prepare 会抢先在新身份上
            // 单集发货，导致下面的集窗批量 0 命中。再拉上门闩，独占首包批量。
            crate::cold_start::rearm();
            let _hold_gate = crate::cold_start::ReleaseOnDrop;
            // 换档前的 warm 打在旧身份上（常 0 命中）。新身份上必须先批量邻集，
            // 再取当前集——先单集发货后再打邻集，服务端对其它 vid 返回空包。
            if let Some((series_id, vid_index)) = series_window {
                log::info!(
                    "[Online] 换档后重跑集窗批量 series={series_id} idx={vid_index}（避免邻集空包）"
                );
                crate::play_service::local::warm_episode_window_gated(
                    state,
                    series_id,
                    vid_index,
                    false,
                )
                .await;
            } else {
                log::warn!(
                    "[Online] 换档后无集窗上下文，只能单集重试（邻集可能空包）"
                );
            }
            // 持闩中：不得 wait，否则与 warm 同理自锁（测侧 8d7cb5b7 独立验挂死点）
            fetch_with_registry(state, vid, progress_key, definition, cap, false).await?
        }
        r => r?,
    };
    let want = play.definition;
    remember_codec(vid, want, &play.codec);
    // 只记「不指定档位」那次解析到的结果：手动选过 720 再点「自动」，
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
    let url = hongguo_protocol::stream_url(vid, want);
    // play 整份 move 进后台填充任务，这里先把要回给调用方的字段取出来
    let result = Prepared {
        url,
        definition: want,
        definitions: play.definitions.clone(),
        codec: play.codec.clone(),
    };
    let settings = state.settings();
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    crate::device::note_play_address(state);
    let state = state.clone();
    crate::runtime::spawn(async move {
        let filled = fill(&state, c, &owned, &owned_key, want, &play, &settings, false).await;
        // 取流权一直持有到数据落盘：中途放开会让并发的第二个请求重新下一遍。
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
        url: hongguo_protocol::stream_url(vid, definition),
        definition,
        definitions,
        codec: remembered_codec(vid, definition),
    })
}

/// 预取一集的流（沉浸流「一切就下一部」的后台铺垫）：提前走完「取流表 + 建渐进流」
/// 两步贵的前置，数据后台渐进填充；用户真滚过去时 `prepare` 命中 ready，首帧只等头部数据区间。
/// 幂等（已有条目/已在取流静默返回）；失败同样静默——预取是锦上添花，不把噪音推给前端。
pub async fn prefetch_stream(state: &AppState, vid: &str, progress_key: &str) -> AppResult<()> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Ok(());
    }
    crate::cold_start::wait().await;
    let c = cache();
    // 已解析过档位且条目在场/在取：无事可做
    if let Some(want) = c.auto_definition(vid)
        && (c.exists(vid, want) || c.is_fetching(vid, want))
    {
        return Ok(());
    }
    // 预取与真实播放同一条选流模式：无 HEVC 机器预取的也必须是 h264，
    // 否则起播时探测结论翻转会命中一条播不了的缓存流。
    let play = match fetch_with_registry(
        state,
        vid,
        progress_key,
        None,
        crate::device::stream_cap(),
        true,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            // 失败被忽略 = 降级（用户播到时现场取流），warn 留痕供排障定案。
            log::warn!("[Online] 预取 {vid} 取流表失败（忽略，播到时现场取）: {e}");
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
    let settings = state.settings();
    let state = state.clone();
    crate::runtime::spawn(async move {
        let filled = fill(&state, c, &owned, &owned_key, want, &play, &settings, true).await;
        c.end_fetch(&owned, want);
        if let Err(e) = filled {
            log::warn!("[Online] 预取填充 {owned} 失败: {e}");
            c.remove(&owned, want);
        }
    });
    Ok(())
}

/// 保证下一集（或任意待播集）的渐进流**已注册**再返回：系统播放器队列预加载只等注册，
/// 不等整集。注册后切集命中 `prepare` 的 ready 分支，首帧只等解码。
/// 不调用 `prepare`（那会把 `cache.current` 改成待播集，让当前集填充让位），只走 `prefetch_stream` 并轮询 `total_len`。
pub async fn ensure_stream_ready(
    state: &AppState,
    vid: &str,
    definition: u32,
    progress_key: &str,
) -> bool {
    const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    if hongguo_stream::total_len(vid, definition).is_some() {
        cache().mark_next(vid, definition);
        // 预取可能早已在头部预算处收工；标记后立刻续填，别让 loader 在填充前沿空等。
        resume_fill(state, vid, definition, progress_key);
        return true;
    }
    let c = cache();
    // 先标记「下一集」：它免于逐出，也免于续填被当成「已切走」而中止；
    // 缓存内容仍受统一预算约束——非当前集只填开头一段，转正后再续满。
    c.mark_next(vid, definition);
    resume_fill(state, vid, definition, progress_key);
    if !c.exists(vid, definition)
        && !c.is_fetching(vid, definition)
        && let Err(error) = prefetch_stream(state, vid, progress_key).await
    {
        log::warn!("[Online] 预注册 {vid}@{definition} 失败: {error}");
    }
    let deadline = std::time::Instant::now() + READY_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if hongguo_stream::total_len(vid, definition).is_some() {
            log::info!("[Online] {vid}@{definition} 流已注册（预注册完成）");
            resume_fill(state, vid, definition, progress_key);
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    log::warn!("[Online] {vid}@{definition} 预注册超时（继续由 loader 等待）");
    false
}

/// 集间流预取的统一入口：同剧连播与换剧都走这里，铺几集由 `Settings.prefetch_episodes`
/// 统一控制（0=全关）。不预取则自动连播每集都要冷走一遍取流表+探针链；
/// 幂等与礼让全部复用 [`prefetch_stream`]。档案里查不到该集的 vid 就放弃（播到时现场解析）。
pub fn schedule_next_episode_prefetch(state: AppState, series_id: &str, next_vid_index: u32) {
    let Some(vid) =
        crate::play_service::local::episode_vid(&state.store, series_id, next_vid_index)
    else {
        // 档案里没有这一集是「预取链整体没生效」的候选现场之一，info 留痕（越界集号属正常兜底）。
        log::info!("[Online] 第 {next_vid_index} 集不在档案里，跳过预取（播到时会现场解析）");
        return;
    };
    let progress_key = format!("{series_id}:{next_vid_index}");
    crate::runtime::spawn(async move {
        // 等位期间当前集可能已经开播并自己取流；prefetch_stream 的幂等闸会让排队者直接退出。
        let _permit = PREFETCH_SLOTS
            .acquire()
            .await
            .expect("预取信号量常驻到进程结束");
        if let Err(e) = prefetch_stream(&state, &vid, &progress_key).await {
            log::warn!("[Online] 第 {next_vid_index} 集预取失败（忽略，播到时现场取）: {e}");
        }
    });
}

/// 等另一个请求把这一档取完，复用它的结果。
/// 播放页对同一集会并发发两次 `play_series`，第二次等数据落进缓存而不是再发一次下载；
/// 取流失败时对方会把条目删掉，这里据此判定「等不到」，按失败返回。
async fn wait_for_fetch(c: &StreamCache, vid: &str, definition: u32) -> AppResult<Prepared> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(20);
    // 上限要大于单次取流耗时，但别无限等：网络挂住时给用户一条明确报错而不是永远转圈。
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

    let start = std::time::Instant::now();
    loop {
        if let Some(hit) = prepared(c, vid, definition) {
            return Ok(hit);
        }
        if !c.exists(vid, definition) {
            return Err(AppError::Network(format!("取流失败: {vid}")));
        }
        if start.elapsed() > DEADLINE {
            return Err(AppError::Network(format!("等待取流超时: {vid}")));
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
