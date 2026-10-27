//! 取流准备与预取编排：解析档位、抢取流权、复用/等待在途取流、连播预取。

use hongguo_common::model::VideoDefinition;
use hongguo_common::{AppError, AppResult};
use hongguo_stream::{StreamCache, cache};

use super::fill::{fill, resume_fill};
use crate::app_state::AppState;

/// 预取（换剧 + 连播共用）的并发上限。每路要先走「取流表 + 探针链」再排
/// 填充门闸；预取档位放开到 10 后，10 路同时探针会在慢网/弱机上抢当前
/// 播放的带宽与 CPU。2 路足够让下一个目标先热，其余排队。
const PREFETCH_CONCURRENCY: usize = 2;
static PREFETCH_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(PREFETCH_CONCURRENCY);

/// 在线播放就绪后带回的档位信息。
pub struct Prepared {
    pub url: String,
    /// 实际选中的档位。请求的档位平台没给时会回退，这里是回退后的真实值。
    pub definition: u32,
    /// 本集提供的全部档位
    pub definitions: Vec<VideoDefinition>,
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
            resume_fill(state, vid, want, progress_key);
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
    // 无 HEVC 扩展的机器选流翻转到 h264（官方 CodecStrategy 语义，见 stream_pick）。
    let cap = hongguo_api::stream_pick::DecoderCapability::from_hevc(
        hongguo_media_platform::capability::playback::hevc_playable(),
    );
    let env = state.api_env();
    let play = hongguo_api::play_url::fetch_play_url(vid, definition, cap, &env).await?;
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
    let url = hongguo_protocol::stream_url(vid, want);
    // play 整份 move 进后台填充任务，这里先把要回给调用方的字段取出来
    let result = Prepared {
        url,
        definition: want,
        definitions: play.definitions.clone(),
    };
    let settings = state.settings();
    let owned = vid.to_string();
    let owned_key = progress_key.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    let state = state.clone();
    crate::runtime::spawn(async move {
        let filled = fill(&state, c, &owned, &owned_key, want, &play, &settings, false).await;
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
        url: hongguo_protocol::stream_url(vid, definition),
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
pub async fn prefetch_stream(state: &AppState, vid: &str, progress_key: &str) -> AppResult<()> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Ok(());
    }
    let c = cache();
    // 已解析过档位且条目在场/在取：无事可做
    if let Some(want) = c.auto_definition(vid)
        && (c.exists(vid, want) || c.is_fetching(vid, want))
    {
        return Ok(());
    }
    // 预取与真实播放同一条选流模式：无 HEVC 机器预取的也必须是 h264，
    // 否则起播时探测结论翻转会命中一条播不了的缓存流。
    let cap = hongguo_api::stream_pick::DecoderCapability::from_hevc(
        hongguo_media_platform::capability::playback::hevc_playable(),
    );
    let env = state.api_env();
    let play = match hongguo_api::play_url::fetch_play_url(vid, None, cap, &env).await {
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

/// 保证下一集（或任意待播集）的渐进流**已注册**再返回：系统播放器队列
/// 预加载只等注册，不等整集。注册前切集只能重建取流链（冷切 0.9s）；
/// 注册后切集命中 `prepare` 的 ready 分支，首帧只等解码。
///
/// 不调用 `prepare`：那会把 `cache.current` 改成待播集，让当前集填充让位。
/// 这里只走 `prefetch_stream`（含 `mark_prefetch`）并轮询 `total_len`。
pub async fn ensure_stream_ready(
    state: &AppState,
    vid: &str,
    definition: u32,
    progress_key: &str,
) -> bool {
    const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    if hongguo_stream::total_len(vid, definition).is_some() {
        cache().mark_next(vid, definition);
        // 预取可能早已在头部预算处收工；标记后立刻续填，别让队列预加载
        // 的 loader 在填充前沿空等 30s。
        resume_fill(state, vid, definition, progress_key);
        return true;
    }
    let c = cache();
    // 先标记「下一集」：它免于逐出（见 `StreamCache::retain_with`），也免于
    // 续填被当成「已切走」而中止（见 `resume_fill` 的让位判定）。缓存内容
    // 仍受统一预算约束——非当前集只填开头一段，转正后再续满。
    c.mark_next(vid, definition);
    resume_fill(state, vid, definition, progress_key);
    if !c.exists(vid, definition)
        && !c.is_fetching(vid, definition)
        && let Err(error) = prefetch_stream(state, vid, progress_key).await
    {
        log::info!("[Online] 预注册 {vid}@{definition} 失败: {error}");
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

/// 集间流预取的统一入口：同剧连播（`resolve_play` 起播后向后铺）与换剧
/// （首页 `play_prefetch` 从入口集起铺）都走这里，铺几集由
/// `Settings.prefetch_episodes` 统一控制（0=全关）。
///
/// 集间切换若不预取，自动连播每集都要冷走一遍取流表+探针链，正是
/// 「下一集黑一两秒」的来源；预取命中则近乎瞬时。
/// 幂等与礼让全部复用 [`prefetch_stream`]（StreamCache 去重 + FillBudget
/// 让位）。档案里查不到该集的 vid 就放弃：播到它时 resolve_play 会
/// 自行解析（与 compat_prefetch 同款语义）。
pub fn schedule_next_episode_prefetch(state: AppState, series_id: &str, next_vid_index: u32) {
    let Some(vid) =
        crate::play_service::local::episode_vid(&state.store, series_id, next_vid_index)
    else {
        return;
    };
    let progress_key = format!("{series_id}:{next_vid_index}");
    crate::runtime::spawn(async move {
        // 等位期间当前集可能已经开播并自己取流；prefetch_stream 的幂等闸
        // （ready / is_fetching）会让排队者直接退出，不会白占填充门闸。
        let _permit = PREFETCH_SLOTS
            .acquire()
            .await
            .expect("预取信号量常驻到进程结束");
        if let Err(e) = prefetch_stream(&state, &vid, &progress_key).await {
            log::info!("[Online] 第 {next_vid_index} 集预取失败（忽略）: {e}");
        }
    });
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
