//! 启动时装配数据：旧档迁移、设置装载、崩溃恢复。

use tauri::Manager;

use crate::app_state::AppState;

/// 装配持久化状态。
///
/// 数据库本体在 `run()` 构造 `AppState` 时就已打开（文件库），
/// 这里负责的是：data.json 一次性迁移 → 设置装载归一化 →
/// 上次中断的合并任务标失败 → 下载队列恢复。
pub fn init(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();

    // 1) 旧 data.json → hongguo.db（已迁移过则是空操作）
    match crate::store::json_migrate::run(&state.store, &crate::store::paths::data_file()) {
        Ok(crate::store::json_migrate::MigrationOutcome::Migrated { .. }) => {}
        Ok(_) => {}
        Err(e) => return Err(tauri::Error::Anyhow(e.into())),
    }

    // 2) 设置装载。启动时读出来的设置必须过一遍归一化：
    // 老版本可能存了越界的并发数
    let stored = state
        .store
        .settings()
        .map_err(|e| tauri::Error::Anyhow(e.into()))?
        .unwrap_or_default();
    let settings = crate::service::settings_service::normalize(stored)
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    let limit = settings.max_concurrency;
    state.replace_settings(settings);

    // 3) 合并在后台线程上跑，进程退出时线程直接没了。启动时还标着 running 的
    // 一定是上次没跑完的：留着会让 UI 上一条「合并中」永远转不完，
    // 而实际早就没人跑了。标成失败让用户看到真实结果。
    let mut interrupted = 0usize;
    let merges = state
        .store
        .merge_tasks()
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    for mut task in merges {
        if task.status == crate::domain::model::MergeStatus::Running {
            task.mark_failed("merge.interrupted");
            task.status = crate::domain::model::MergeStatus::Failed;
            state
                .store
                .upsert_merge_task(&task)
                .map_err(|e| tauri::Error::Anyhow(e.into()))?;
            interrupted += 1;
        }
    }
    if interrupted > 0 {
        log::info!("[Store] {interrupted} 个合并任务因上次退出被中断，已标为失败");
    }

    // 4) 下载队列恢复：running 视为中断，改回 pending 续跑（语义在 queue 层）
    let tasks = state
        .store
        .tasks()
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    let task_count = tasks.len();
    let series_count = state
        .store
        .series_all()
        .map(|s| s.len())
        .unwrap_or_default();
    let queue = crate::service::download_service::queue::DownloadQueue::restore(tasks, limit);
    state.replace_queue(queue);

    // 加载结果必须能被人核对：设置读不出来时用户只会看到「目录变回默认了」这种
    // 症状，没有日志根本无从判断是加载失败还是本来就没存过。
    let settings = state.settings();
    log::info!(
        "[Store] {} | 目录={} 并发={} 命名={:?} 剧集={} 任务={}",
        crate::store::paths::db_file().display(),
        settings.download_dir,
        settings.max_concurrency,
        settings.naming,
        series_count,
        task_count
    );

    Ok(())
}
