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

/// 缺少条目类型时先拉本剧分集档案。同一次取流沿用同一设备快照。
async fn fetch_with_registry(
    state: &AppState,
    vid: &str,
    progress_key: &str,
    definition: Option<u32>,
    cap: hongguo_api::stream_pick::DecoderCapability,
) -> AppResult<hongguo_api::play_url::PlayInfo> {
    crate::cold_start::wait(state).await?;
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
    pub automatic_definition: bool,
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
) -> AppResult<Prepared> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    crate::cold_start::wait(state).await?;
    let c = cache();
    // 记下当前集：逐出策略保它、预填礼让它、切走后旧填充据此自杀。
    // 一次只看一集的**语义下有界保留**：全清的话，信息流切剧目标一弹，
    // 正在被 `<video>` 读的那条流就被逐掉，播放追上缓冲前沿只能超时。
    // set_current 内已跑 retain_playing_set（连播接力同路，不依赖 prepare）。
    c.set_current(vid);

    // 还没有键可查：先做一次很小的接口调用把流表拿回来，解析出的真实档位才是缓存的键；
    // 贵的那一步（整集下载 + 解密）要等拿到取流权之后才发生。
    let cap = crate::device::stream_cap();
    fetch_with_registry(state, vid, progress_key, definition, cap).await?;
    let selection = super::super::resolution::select(
        state,
        vid,
        progress_key,
        definition,
        hongguo_api::resolution_policy::Purpose::Playback,
    )
    .await?;
    let automatic_definition = selection.automatic;
    let play = selection.play;
    let want = play.definition;
    // 记真实自动结果供队列查询；它不再跳过下一次的动态决策。
    if definition.is_none() {
        c.remember_auto(vid, want);
    }
    if let Some(hit) = prepared(c, vid, want, automatic_definition) {
        resume_fill(state, vid, want, progress_key);
        return Ok(hit);
    }
    log::info!(
        "[Online] {vid} 选中档位 {want}（共 {} 档可选）",
        play.definitions.len()
    );

    if !c.begin_fetch(vid, want) {
        // 抢取流权时又被别人插队（窗口极窄），转为等待
        let result = wait_for_fetch(c, vid, want, automatic_definition).await?;
        resume_fill(state, vid, want, progress_key);
        return Ok(result);
    }
    c.set_definitions(vid, want, &play.definitions);
    remember_codec(vid, want, &play.codec);
    let url = hongguo_protocol::stream_url(vid, want);
    // play 整份 move 进后台填充任务，这里先把要回给调用方的字段取出来
    let result = Prepared {
        url,
        definition: want,
        automatic_definition,
        definitions: play.definitions.clone(),
        codec: play.codec.clone(),
    };
    let settings = state.settings();
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    let state = state.clone();
    crate::runtime::spawn(async move {
        let filled = fill(&state, c, &owned, &owned_key, want, &play, &settings, None).await;
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
fn prepared(
    c: &StreamCache,
    vid: &str,
    definition: u32,
    automatic_definition: bool,
) -> Option<Prepared> {
    c.ready(vid, definition).map(|definitions| Prepared {
        url: hongguo_protocol::stream_url(vid, definition),
        definition,
        automatic_definition,
        definitions,
        codec: remembered_codec(vid, definition),
    })
}

/// 预取一集的流（沉浸流「一切就下一部」的后台铺垫）：提前走完「取流表 + 建渐进流」
/// 两步贵的前置，数据后台渐进填充；用户真滚过去时 `prepare` 命中 ready，首帧只等头部数据区间。
/// 幂等（已有条目/已在取流静默返回）；失败同样静默——预取是锦上添花，不把噪音推给前端。
pub async fn prefetch_stream(
    state: &AppState,
    vid: &str,
    progress_key: &str,
    bytes: u64,
) -> AppResult<()> {
    let vid = vid.trim();
    if vid.is_empty() || bytes == 0 {
        return Ok(());
    }
    crate::cold_start::wait(state).await?;
    let c = cache();
    // 预取与起播共用内置解码约束，缓存中不放入无法播放的流。
    match fetch_with_registry(state, vid, progress_key, None, crate::device::stream_cap()).await {
        Ok(_) => {}
        Err(e) => {
            // 失败被忽略 = 降级（用户播到时现场取流），warn 留痕供排障定案。
            log::warn!("[Online] 预取 {vid} 取流表失败（忽略，播到时现场取）: {e}");
            return Ok(());
        }
    };
    let play = super::super::resolution::select(
        state,
        vid,
        progress_key,
        None,
        hongguo_api::resolution_policy::Purpose::Prefetch,
    )
    .await?
    .play;
    let want = play.definition;
    c.remember_auto(vid, want);
    if c.is_fetching(vid, want) || c.exists(vid, want) {
        return Ok(());
    }
    if !c.begin_fetch(vid, want) {
        return Ok(());
    }
    c.set_definitions(vid, want, &play.definitions);
    remember_codec(vid, want, &play.codec);
    // 显式标记预取：keep_only 只豁免这一条，别让在途填充占住串行队列
    c.mark_prefetch(vid, want);
    log::info!("[Online] 预取 {vid} 档位 {want}，后台渐进填充开始");
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    let c = cache();
    let settings = state.settings();
    let state = state.clone();
    crate::runtime::spawn(async move {
        let filled = fill(
            &state,
            c,
            &owned,
            &owned_key,
            want,
            &play,
            &settings,
            Some(bytes),
        )
        .await;
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
    bytes: u64,
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
        && let Err(error) = prefetch_stream(state, vid, progress_key, bytes).await
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
    log::debug!("[Online] {vid}@{definition} 预注册超时，切集后现场取流");
    false
}

/// 同剧连播与换剧共用的单集媒体预取入口。
/// 幂等与礼让全部复用 [`prefetch_stream`]。档案里查不到该集的 vid 就放弃（播到时现场解析）。
pub fn schedule_next_episode_prefetch(
    state: AppState,
    series_id: &str,
    next_vid_index: u32,
    bytes: u64,
) {
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
        if let Err(e) = prefetch_stream(&state, &vid, &progress_key, bytes).await {
            log::warn!("[Online] 第 {next_vid_index} 集预取失败（忽略，播到时现场取）: {e}");
        }
    });
}

/// 等另一个请求把这一档取完，复用它的结果。
/// 播放页对同一集会并发发两次 `play_series`，第二次等数据落进缓存而不是再发一次下载；
/// 取流失败时对方会把条目删掉，这里据此判定「等不到」，按失败返回。
async fn wait_for_fetch(
    c: &StreamCache,
    vid: &str,
    definition: u32,
    automatic_definition: bool,
) -> AppResult<Prepared> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(20);
    // 上限要大于单次取流耗时，但别无限等：网络挂住时给用户一条明确报错而不是永远转圈。
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

    let start = std::time::Instant::now();
    loop {
        if let Some(hit) = prepared(c, vid, definition, automatic_definition) {
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
