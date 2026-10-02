//! 启动时加载数据文件。

use tauri::Manager;

use crate::app_state::AppState;

/// 把磁盘数据装进全局状态。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let path = crate::store::paths::data_file();
    let data = crate::store::DataStore::load(&path);
    // 启动时读出来的设置必须过一遍归一化：老版本可能存了越界的并发数
    let settings = crate::service::settings_service::normalize(data.settings.clone())
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    let limit = settings.max_concurrency;

    *state.store.write() = data;
    state.replace_settings(settings);

    // 合并在后台线程上跑，进程退出时线程直接没了。启动时还标着 running 的
    // 一定是上次没跑完的：留着会让 UI 上一条「合并中」永远转不完，
    // 而实际早就没人跑了。标成失败让用户看到真实结果。
    let interrupted = {
        let mut data = state.store.write();
        let mut n = 0;
        for task in data.merge_tasks.iter_mut() {
            if task.status == crate::domain::model::MergeStatus::Running {
                task.mark_failed("merge.interrupted");
                task.status = crate::domain::model::MergeStatus::Failed;
                n += 1;
            }
        }
        if n > 0 {
            let _ = data.save(&path);
        }
        n
    };
    if interrupted > 0 {
        log::info!("[Store] {interrupted} 个合并任务因上次退出被中断，已标为失败");
    }

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
