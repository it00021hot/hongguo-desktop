//! service 事件订阅的启动接线。
//!
//! 顺序要求：本步必须早于 [`crate::bootstrap::downloader`]——启动恢复会立即触发下载调度，进度事件从那一刻起就会发布。

use tauri::Manager;

/// 订阅 service 事件并转发为前端事件。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app
        .state::<hongguo_service::app_state::AppState>()
        .inner()
        .clone();
    crate::events::subscribe(app, &state);
    Ok(())
}
