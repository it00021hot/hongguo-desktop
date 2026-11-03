//! 下载调度的启动续跑。

use tauri::Manager;

use hongguo_service::app_state::AppState;

/// 把待跑任务重新推入调度。队列由 [`crate::bootstrap::store`] 装配阶段恢复（`Running` 在那里改回 `Pending`）；这里只负责按下 `kick`——恢复完不调度的话，任务会一直挂着直到用户手动点「一键启动」。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>().inner().clone();
    let status = state.queue().status();
    if status.pending > 0 {
        log::info!("[Bootstrap] 待续跑任务 {} 个，已推入调度", status.pending);
        hongguo_service::download_service::kick(state);
    }
    Ok(())
}
