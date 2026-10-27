//! 播放。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_common::model::{PlayRequest, PlayResponse};
use hongguo_service::app_state::AppState;

/// 解析播放请求（本地成品优先，其次在线流）。
///
/// 取流编排与进度事件全在 service：运行时共用（`hongguo_service::runtime`）、
/// 进度走事件总线（`online-play-progress` 由壳的事件转发器发给前端）。
#[tauri::command]
pub async fn play_series(
    state: State<'_, AppState>,
    request: PlayRequest,
) -> AppResult<PlayResponse> {
    hongguo_service::play_service::local::resolve_play(state.inner(), &request).await
}

/// 预取一部剧的在线流（沉浸流刷剧：前端按预取档位对**后续每部**各调一次）。
///
/// 幂等且静默：本地没档案就先解析登记（通常前端已预取过分集，这里秒回），
/// 流已取/在取直接返回；取流失败不报错——预取是优化，失败大不了现场取。
/// 每部只铺入口集（只填开头一段，转正当前集才续满整集）；同剧后续集的
/// 连播铺集由起播的 [`schedule_following_episodes`] 负责，这里不重复。
/// 档位 0=全关（含换剧预取）。
#[tauri::command]
pub async fn play_prefetch(state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    if state.settings().prefetch_episodes == 0 {
        return Ok(());
    }
    let series = match state.store.series_by_id(&series_id)? {
        Some(s) => s,
        None => {
            let env = state.api_env();
            let s =
                hongguo_service::series_service::resolver::resolve_series(&series_id, &env).await?;
            hongguo_service::series_service::registry::upsert_and_persist(&state, s.clone())?;
            s
        }
    };
    // 起点与首页同一份续播决策（resolve_entry；传空历史 = 只看本地断点）：
    // 看过 >3 秒的剧预取上次看到的那一集；没看过的回落第 1 集。入口集
    // 已不在分集表里（下架/重排）也回落第 1 集。
    let entry = hongguo_service::play_service::resume::resolve_entry(&state.store, &series_id, &[])
        .map(|(idx, _)| idx)?;
    let entry = if series.episodes.iter().any(|e| e.vid_index == entry) {
        entry
    } else {
        1
    };
    hongguo_service::play_service::online::schedule_next_episode_prefetch(
        state.inner().clone(),
        &series_id,
        entry,
    );
    Ok(())
}

/// 同步 command 会占 **Tauri 主线程**——播放时每 5 秒一次的进度保存
/// 与其它 IPC 在主线程排队，是「窗口未响应」的候选元凶。这里改 async
/// 并把 DB 查询扔进阻塞线程池，主线程只做调度。
#[tauri::command]
pub async fn save_playback_position(
    state: State<'_, AppState>,
    series_id: String,
    vid_index: u32,
    current_time: f64,
    duration: f64,
) -> AppResult<()> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        hongguo_service::play_service::position::save_store(
            &store,
            &series_id,
            vid_index,
            current_time,
            duration,
        )
    })
    .await
    .map_err(|e| hongguo_common::AppError::Internal(format!("保存播放进度失败: {e}")))?
}

/// 一部剧最近看到的那一集（详情页「继续看」的数据源）。
///
/// 没看过（playback 表无记录）返回 None，前端回落云端观看历史。
/// 读是单行查询，但同样走阻塞线程池，不给主线程添堵。
#[tauri::command]
pub async fn series_progress(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Option<hongguo_common::model::SeriesProgress>> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.series_last_position(&series_id))
        .await
        .map_err(|e| hongguo_common::AppError::Internal(format!("读取播放进度失败: {e}")))?
        .map(|hit| {
            hit.map(|(vid_index, p)| hongguo_common::model::SeriesProgress {
                vid_index,
                current_time: p.current_time,
                duration: p.duration,
                updated_at: p.updated_at,
            })
        })
}
