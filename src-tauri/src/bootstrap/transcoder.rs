//! 转码能力探测（硬解是否可用）。

use tauri::Manager;

/// 启动时探测一次并缓存结果。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let _ = app.state::<crate::app_state::AppState>();
    crate::media::capability::probe::warm_up();
    Ok(())
}
