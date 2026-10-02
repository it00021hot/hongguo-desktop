//! 恢复未完成的下载任务。

use tauri::Manager;

/// 启动时把未完成任务重新排队（记录丢失时靠 rescan 自愈）。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<crate::app_state::AppState>();
    let status = state.queue().status();
    if status.pending > 0 {
        log::info!("[Bootstrap] 待续跑任务 {} 个", status.pending);
    }
    Ok(())
}
