//! 快速合并：流复制拼接。

use std::path::PathBuf;

use super::done_inputs;
use super::guard::RunningMerge;
use super::progress::ProgressSink;
use crate::app_state::AppState;
use hongguo_common::model::MergeTask;
use hongguo_common::{AppError, AppResult};

/// 快速合并某剧已下载的分集。流复制没有「逐集」的天然边界——`concat_copy` 一次拼完，
/// 只在开始前与结束后各报一次进度；取消只能在开写前检查一次（拼接中途打断会留下半截文件）。
pub fn quick_merge(
    state: &AppState,
    series_id: &str,
    output_name: &str,
    task: &MergeTask,
    on_progress: &ProgressSink,
    slot: &RunningMerge,
) -> AppResult<(PathBuf, u64, usize)> {
    let inputs = done_inputs(state, series_id);

    // 闸门放在执行点上，不放在 UI 上：绕过界面直接提交的命令请求同样要拦住，
    // 否则产出的就是一份索引都过不去的文件。
    ensure_codec_consistent(&inputs)?;

    if slot.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    let settings = state.settings();
    let output = super::output_file(&settings, output_name);

    let total = inputs.len();
    on_progress(0.0, total, task);

    let sorted: Vec<PathBuf> = inputs.into_iter().map(|(_, p)| p).collect();
    let (size, count) = hongguo_media::remux::concat_copy(&sorted, &output)?;
    on_progress(total as f64, total, task);
    Ok((output, size, count))
}

/// 编码不一致就拒绝，不产出文件：`concat_copy` 是整文件字节级顺序拼接，
/// 各集容器结构或编码参数对不上时产物连索引都过不去。
fn ensure_codec_consistent(inputs: &[(u32, PathBuf)]) -> AppResult<()> {
    if let Some(episode) = hongguo_media::codec_probe::check(inputs).mismatch_episode {
        return Err(AppError::Media(format!(
            "第 {episode} 集的编码与第 1 集不一致，快速合并会产出损坏文件"
        )));
    }
    Ok(())
}
