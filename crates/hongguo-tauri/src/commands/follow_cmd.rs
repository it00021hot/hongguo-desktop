//! 追更 command：追更开关 / 列表 / 是否已追 / 更新提醒（列表、已读、清空、检查）。
//!
//! 写操作走 service（登录墙 + 基线维护在 `follow_service`）；通知列表/已读/清空是纯本地 store 读写。前端未登录时应禁用入口（同互动域口径）。

use tauri::State;

use hongguo_api::interact::FollowUpdatePage;
use hongguo_common::model::FollowNotice;
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;
use hongguo_service::follow_service;

/// 追更 / 取消追更一部剧。
#[tauri::command]
pub async fn follow_update(
    state: State<'_, AppState>,
    series_id: String,
    follow: bool,
) -> AppResult<()> {
    follow_service::toggle(&state, &series_id, follow).await
}

/// 追更列表一页（cursor 翻页；首页空串）。
#[tauri::command]
pub async fn follow_updates(
    state: State<'_, AppState>,
    cursor: Option<String>,
) -> AppResult<FollowUpdatePage> {
    follow_service::list(&state, cursor.as_deref().unwrap_or("")).await
}

/// 是否已追某剧（追更列表第一页本地判成员）。
#[tauri::command]
pub async fn is_following(state: State<'_, AppState>, series_id: String) -> AppResult<bool> {
    follow_service::is_following(&state, &series_id).await
}

/// 更新提醒列表（本地库，新→旧）。
#[tauri::command]
pub fn follow_notices(state: State<'_, AppState>) -> AppResult<Vec<FollowNotice>> {
    follow_service::notices(&state)
}

/// 标记提醒已读，返回改动行数。
#[tauri::command]
pub fn follow_notices_read(state: State<'_, AppState>, ids: Vec<i64>) -> AppResult<u64> {
    follow_service::mark_notices_read(&state, &ids)
}

/// 清空提醒。
#[tauri::command]
pub fn follow_notices_clear(state: State<'_, AppState>) -> AppResult<()> {
    follow_service::clear_notices(&state)
}

/// 手动检查更新（设置/提醒页的「检查更新」按钮）。未登录报错（与定时器的
/// 静默跳过不同：这是用户主动动作，要有回音）。
#[tauri::command]
pub async fn follow_check_updates(state: State<'_, AppState>) -> AppResult<usize> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("登录后才能追更".into()));
    }
    follow_service::check_updates(&state).await
}
