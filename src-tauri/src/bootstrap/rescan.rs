//! 启动时从磁盘补回丢失的任务记录。
//!
//! data.json 被清掉 / 损坏备份后任务全没了，但磁盘上的分集文件还在——
//! 不补的话用户看到的就是「明明下过却全是空的」。放在 downloader 之前：
//! 补回来的都是已完成任务，不影响调度，只影响列表展示的完整性。

use tauri::Manager;

use crate::app_state::AppState;

pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    match crate::service::download_service::rescan::rescan_from_disk(&state) {
        Ok(summary) => {
            if summary.added.is_empty() {
                log::info!(
                    "[Bootstrap] 磁盘扫描无新增（扫过 {} 部剧）",
                    summary.scanned_series
                );
            } else {
                log::info!(
                    "[Bootstrap] 从磁盘补回 {} 条下载记录（扫过 {} 部剧）",
                    summary.added.len(),
                    summary.scanned_series
                );
            }
        }
        // 扫描失败不拦启动：这只是自愈手段，坏了顶多维持现状
        Err(e) => log::warn!("[Bootstrap] 磁盘扫描补登记失败: {e}"),
    }
    Ok(())
}
