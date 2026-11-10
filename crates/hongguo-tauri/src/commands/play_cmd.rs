//! 播放。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_common::model::{PlayRequest, PlayResponse};
use hongguo_service::app_state::AppState;

/// 解析播放请求（本地成品优先，其次在线流）。取流编排与进度事件全在 service：进度走事件总线（`online-play-progress` 由壳的事件转发器发给前端）。
#[tauri::command]
pub async fn play_series(
    state: State<'_, AppState>,
    request: PlayRequest,
) -> AppResult<PlayResponse> {
    hongguo_service::play_service::local::resolve_play(state.inner(), &request).await
}

/// 预取首页候选剧的入口流。
#[tauri::command]
pub async fn play_prefetch(
    state: State<'_, AppState>,
    series_ids: Vec<String>,
) -> AppResult<Vec<String>> {
    hongguo_service::play_service::prefetch_feed(state.inner(), series_ids, &[]).await
}

/// 同步 command 会占 **Tauri 主线程**——播放时每 5 秒一次的进度保存与其它 IPC 在主线程排队，是「窗口未响应」的候选元凶；改 async 并把 DB 查询扔进阻塞线程池，主线程只做调度。
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

/// 一部剧最近看到的那一集（详情页「继续看」的数据源）。没看过（playback 表无记录）返回 None，前端回落云端观看历史；同样走阻塞线程池，不给主线程添堵。
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
