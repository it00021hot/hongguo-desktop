//! 启动内嵌浏览器嗅探窗口（懒创建，首次搜索时才真正开窗）。

use tauri::Manager;

/// 预热嗅探模块。窗口本身在首次 `search_series` 时懒创建。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let _ = app.state::<crate::app_state::AppState>();
    Ok(())
}
