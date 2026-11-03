//! 启动时从磁盘补回丢失的任务记录（后台跑，不占启动关键路径）。
//!
//! 损坏备份后任务全没了，但磁盘上的分集文件还在——不补的话用户看到的就是「明明下过却全是空的」。补回的都是已完成任务、不影响调度只影响列表展示：走 `actions::rescan_downloads`（设置页手动入口同款），逐条发 `download-task-added` 事件，任务页开着也能实时看到。

use tauri::Manager;

use hongguo_service::app_state::AppState;

pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>().inner().clone();
    // 阻塞型磁盘扫描进 spawn_blocking 别占 worker；失败不拦启动——这只是自愈手段，坏了顶多维持现状
    tauri::async_runtime::spawn_blocking(move || {
        match hongguo_service::download_service::actions::rescan_downloads(&state) {
            Ok(0) => log::info!("[Bootstrap] 磁盘扫描无新增"),
            Ok(added) => log::info!("[Bootstrap] 从磁盘补回 {added} 条下载记录"),
            Err(e) => log::warn!("[Bootstrap] 磁盘扫描补登记失败: {e}"),
        }
    });
    Ok(())
}
