//! 播放。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::{PlayRequest, PlayResponse};
use crate::error::AppResult;

#[tauri::command]
pub async fn play_series(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: PlayRequest,
) -> AppResult<PlayResponse> {
    crate::service::play_service::local::resolve_play(&app, &state, &request).await
}

/// 预取一部剧第 1 集的在线流（沉浸流「一切就下一部」）。
///
/// 幂等且静默：本地没档案就先解析登记（通常前端已预取过分集，这里秒回），
/// 流已取/在取直接返回；取流表失败不报错——预取是优化，失败大不了现场取。
#[tauri::command]
pub async fn play_prefetch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<()> {
    let series = match state.store.series_by_id(&series_id)? {
        Some(s) => s,
        None => {
            let env = state.api_env();
            let s = crate::service::series_service::resolver::resolve_series(&series_id, &env)
                .await?;
            crate::service::series_service::registry::upsert_and_persist(&state, s.clone())?;
            s
        }
    };
    let Some(ep) = series.episodes.iter().find(|e| e.vid_index == 1) else {
        return Ok(());
    };
    if ep.vid.is_empty() {
        return Ok(());
    }
    let settings = state.settings();
    let env = state.api_env();
    crate::service::play_service::online::prefetch_stream(&app, &ep.vid, &settings, &env).await
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
        crate::service::play_service::position::save_store(
            &store,
            &series_id,
            vid_index,
            current_time,
            duration,
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Network(format!("保存播放进度失败: {e}")))?
}
