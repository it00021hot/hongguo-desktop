//! 观看历史 command：云端（登录态）与本地（离线可记可管，issue #10）。

use serde::Deserialize;

use hongguo_api::history::{WatchHistoryPage, delete_watch_history, fetch_watch_history};
use hongguo_common::model::LocalHistoryItem;
use hongguo_common::utils::time::now_ms;
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;
use tauri::State;

/// 批量删除历史的单条引用（book_id/vid 定位，vid_index 随条带上）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistoryDeleteRef {
    pub series_id: String,
    pub vid: String,
    pub vid_index: i64,
}

/// 拉一页云端观看历史（官方 App 侧栏「历史」同源）。
#[tauri::command]
pub async fn watch_history_list(
    state: State<'_, AppState>,
    offset: Option<i64>,
) -> AppResult<WatchHistoryPage> {
    let env = state.api_env();
    fetch_watch_history(offset.unwrap_or(0), &env).await
}

/// 批量删除云端观看历史（read_history/update 的 is_delete+use_soft_delete
/// 形态）。
#[tauri::command]
pub async fn watch_history_delete(
    state: State<'_, AppState>,
    items: Vec<WatchHistoryDeleteRef>,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("历史删除需要先登录".into()));
    }
    let env = state.api_env();
    let refs: Vec<(String, String, i64)> = items
        .into_iter()
        .map(|it| (it.series_id, it.vid, it.vid_index))
        .collect();
    let n = refs.len();
    delete_watch_history(&refs, &env).await?;
    log::info!("[History] 批量删除 {n} 条观看历史");
    Ok(())
}

/// 记录观看历史（issue #10）：本地必记（未登录也有历史、可管理），登录态
/// 顺路上报云端——节拍由播放器的上报心跳按播放位置判定（`history-cadence.ts`）：
/// 新剧 5 秒首报 / 同剧切集立即翻页 / 每 60 秒一拍 / 暂停·切集·退出 force 补记；
/// 上报失败不影响本地行（reported=0 留待补传）。cloud_report_progress 的
/// 继任者：云端语义不变，多了本地一行。
#[tauri::command]
pub async fn record_watch_history(
    state: State<'_, AppState>,
    series_id: String,
    vid: String,
    vid_index: u32,
    position_ms: i64,
    duration_ms: i64,
    meta: Option<RecordHistoryMeta>,
) -> AppResult<()> {
    let meta = meta.unwrap_or(RecordHistoryMeta {
        title: String::new(),
        cover: String::new(),
        episode_cnt: 0,
    });
    let item = LocalHistoryItem {
        series_id,
        title: meta.title,
        cover: meta.cover,
        vid_index,
        vid,
        position_ms: position_ms.max(0),
        duration_ms: duration_ms.max(0),
        episode_cnt: meta.episode_cnt,
        updated_at_ms: now_ms(),
    };
    hongguo_service::history_service::record(state.inner(), &item).await
}

/// 记录时的展示元数据（剧名/封面/总集数）：播放器手里现成，落进本地行
/// 免得历史页未登录模式还得逐剧拉详情。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordHistoryMeta {
    title: String,
    cover: String,
    episode_cnt: u32,
}

/// 本地观看历史（未登录模式的历史页数据源；字段与云端条目同形）。
#[tauri::command]
pub async fn local_history_list(state: State<'_, AppState>) -> AppResult<Vec<LocalHistoryItem>> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.local_history_list())
        .await
        .map_err(|e| AppError::Internal(format!("本地历史读取失败: {e}")))?
}

/// 批量删除本地观看历史（按剧，与历史页卡片/批量管理同粒度）。
#[tauri::command]
pub async fn local_history_delete(
    state: State<'_, AppState>,
    series_ids: Vec<String>,
) -> AppResult<()> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.delete_local_history(&series_ids))
        .await
        .map_err(|e| AppError::Internal(format!("本地历史删除失败: {e}")))?
}
