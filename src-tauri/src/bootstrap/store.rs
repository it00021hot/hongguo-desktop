//! 启动时加载数据文件。

use tauri::Manager;

use crate::app_state::AppState;

/// 把磁盘数据装进全局状态。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let path = crate::store::paths::data_file();
    let data = crate::store::DataStore::load(&path);
    let settings = crate::service::settings_service::normalize(data.settings.clone());
    let limit = settings.max_concurrency;

    *state.store.write() = data;
    state.replace_settings(settings);

    let queue = crate::service::download_service::queue::DownloadQueue::restore(
        state.store.read().tasks.clone(),
        limit,
    );
    state.replace_queue(queue);

    // 加载结果必须能被人核对：设置读不出来时用户只会看到「目录变回默认了」这种
    // 症状，没有日志根本无从判断是加载失败还是本来就没存过。
    let store = state.store.read();
    let settings = state.settings();
    log::info!(
        "[Store] {} | 目录={} 并发={} 命名={:?} 剧集={} 任务={}",
        path.display(),
        settings.download_dir,
        settings.max_concurrency,
        settings.naming,
        store.series.len(),
        store.tasks.len()
    );

    Ok(())
}
