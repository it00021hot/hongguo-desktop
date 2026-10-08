//! 文件清理。

use tauri::State;

use crate::app_state::AppState;
use crate::error::AppResult;

/// 删除某部剧的全部本地文件（含合并产物与残留临时文件，空目录一并删除）。
pub fn delete_series(state: &State<'_, AppState>, series_id: &str) -> AppResult<usize> {
    let tasks = state.queue().of_series(series_id);
    // 目录推导用任务快照里的剧名（与合并服务同源），不查剧集档案
    let title = tasks
        .first()
        .map(|t| t.series_title.clone())
        .unwrap_or_default();
    let mut n = 0;
    let mut removed_ids: Vec<String> = Vec::new();
    for t in tasks.iter().filter(|t| t.is_done()) {
        let p = std::path::Path::new(&t.file_path);
        if std::fs::remove_file(p).is_ok() {
            n += 1;
        }
        // 记录无条件跟着摘：本函数跑完，这些集在磁盘上已经没有文件了。
        // 留着 completed 记录，播放会照它去开本地文件 → 「媒体处理失败」，
        // 回落不到在线流。文件本来就缺（记录早发霉）时同样要摘，那是自愈。
        removed_ids.push(t.id.clone());
        crate::service::download_service::worker::cleanup_temp(p);
    }
    state.queue().remove(&removed_ids);
    cleanup_merge_artifacts(state, &title);
    remove_empty_series_dir(state, &title);
    Ok(n)
}

/// 删除单集文件。
pub fn delete_episode(
    state: &State<'_, AppState>,
    series_id: &str,
    vid_index: u32,
) -> AppResult<bool> {
    let path = state.queue().completed_path(series_id, vid_index);
    match path {
        Some(p) => {
            let path = std::path::Path::new(&p);
            let ok = std::fs::remove_file(path).is_ok();
            // 记录必须跟文件一起没。留着 completed 记录，播放会照它去开本地文件，
            // 而文件已经不在 → 直接「媒体处理失败」，回落不到在线流。
            // 文件本来就缺（记录早发霉）时同样要摘，那是自愈。
            state.queue().remove_episode(series_id, vid_index);
            crate::service::download_service::worker::cleanup_temp(path);
            Ok(ok)
        }
        None => Ok(false),
    }
}

/// 删除全部已下载。遍历范围同样来自任务记录：档案里有而磁盘上没下载过
/// 的剧，清理无从谈起。
pub fn delete_all(state: &State<'_, AppState>) -> AppResult<usize> {
    let mut ids: Vec<String> = Vec::new();
    for t in state.queue().all().iter().filter(|t| t.is_done()) {
        if !ids.contains(&t.series_id) {
            ids.push(t.series_id.clone());
        }
    }
    let mut n = 0;
    for id in &ids {
        n += delete_series(state, id)?;
    }
    Ok(n)
}

/// 删除某剧的合并产物与残留 concat 列表。
fn cleanup_merge_artifacts(state: &State<'_, AppState>, title: &str) {
    let dir = state.settings().series_dir(title);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.contains("合集") || name.ends_with(".ffconcat.txt") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 空目录一并删除。
fn remove_empty_series_dir(state: &State<'_, AppState>, title: &str) {
    let dir = state.settings().series_dir(title);
    if std::fs::read_dir(&dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir_all(&dir);
    }
}
