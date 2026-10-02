//! 快速合并：流复制拼接。

use std::path::PathBuf;

use tauri::State;

use super::progress::ProgressSink;
use crate::app_state::AppState;
use crate::domain::model::MergeTask;
use crate::error::AppResult;

/// 快速合并某剧已下载的分集。
///
/// 流复制没有「逐集」的天然边界——`concat_copy` 一次拼完，所以这里只在
/// 开始前与结束后各报一次进度。
pub fn quick_merge(
    state: &State<'_, AppState>,
    series_id: &str,
    output_name: &str,
    task: &MergeTask,
    on_progress: &ProgressSink,
) -> AppResult<(PathBuf, u64, usize)> {
    let queue = state.queue();
    let tasks = queue.of_series(series_id);
    let mut inputs: Vec<(u32, PathBuf)> = tasks
        .iter()
        .filter(|t| t.is_done())
        .map(|t| (t.vid_index, PathBuf::from(&t.file_path)))
        .filter(|(_, p)| p.exists())
        .collect();
    inputs = crate::media::remux::sort_by_index(&inputs);

    let settings = state.settings();
    let dir = settings.series_dir(output_name);
    let output = dir.join(format!("{output_name} 合集.mp4"));

    let total = inputs.len();
    on_progress(0, total, task);

    let sorted: Vec<PathBuf> = inputs.into_iter().map(|(_, p)| p).collect();
    let (size, count) = crate::media::remux::concat_copy(&sorted, &output)?;
    on_progress(total, total, task);
    Ok((output, size, count))
}
