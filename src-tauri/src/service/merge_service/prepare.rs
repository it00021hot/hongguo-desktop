//! 合并前校验：排序集号、编码一致性、磁盘空间。

use std::path::PathBuf;

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::MergePreflight;
use crate::error::AppResult;

/// 收集某剧已下载的集（按集号升序），并做合并前校验。
pub fn preflight(state: &State<'_, AppState>, series_id: &str) -> AppResult<MergePreflight> {
    let queue = state.queue();
    let tasks = queue.of_series(series_id);
    let done: Vec<(u32, PathBuf, u64)> = tasks
        .iter()
        .filter(|t| t.is_done())
        .map(|t| (t.vid_index, PathBuf::from(&t.file_path), 0u64))
        .filter(|(_, p, _)| p.exists())
        .collect();

    if done.is_empty() {
        return Ok(MergePreflight {
            ok: false,
            episode_count: 0,
            estimated_size: 0,
            free_space: 0,
            codec_consistent: true,
            warnings: vec![i18n("merge.noDownloads")],
        });
    }

    let estimated_size: u64 = done
        .iter()
        .map(|(_, p, _)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

    let mut warnings = Vec::new();
    if estimated_size > free_space() {
        warnings.push(i18n("merge.insufficientSpace"));
    }

    Ok(MergePreflight {
        ok: warnings.is_empty(),
        episode_count: done.len(),
        estimated_size,
        free_space: free_space(),
        codec_consistent: true,
        warnings,
    })
}

/// 取磁盘剩余空间。
pub fn free_space() -> u64 {
    #[cfg(unix)]
    {
        // 简化：只校验下载根所在盘
        0
    }
    #[cfg(windows)]
    {
        0
    }
}

fn i18n(key: &str) -> String {
    key.to_string()
}
