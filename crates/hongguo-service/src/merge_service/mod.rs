//! 合并服务：快速合并（流复制）与兼容合并（转码）。三条路径（前置校验、快速合并、兼容合并）
//! 的「哪些集真的可以参与合并」统一由 [`done_inputs`] 给出——三处各写一遍迟早漂成三种口径。

pub mod compat;
pub mod guard;
pub mod prepare;
pub mod progress;
pub mod quick;
pub mod start;

use std::path::{Path, PathBuf};

use crate::app_state::AppState;
use hongguo_common::model::MergeCandidate;
use hongguo_common::{AppError, AppResult};

/// 某剧可参与合并的分集，按集号升序。「已完成」与「文件存在」必须同时成立：
/// 用户在系统里删过文件时任务记录仍留着 completed，只看状态就会把不存在的路径交给拼接器。
pub fn done_inputs(state: &AppState, series_id: &str) -> Vec<(u32, PathBuf)> {
    hongguo_media::remux::sort_by_index(
        &state
            .queue()
            .of_series(series_id)
            .iter()
            .filter(|t| t.is_done())
            .map(|t| (t.vid_index, PathBuf::from(&t.file_path)))
            .filter(|(_, p)| p.exists())
            .collect::<Vec<_>>(),
    )
}

/// 合并产物路径：`<剧名目录>/<剧名> 合集.mp4`。quick/compat 两条链同一
/// 份构造——这串路径同时是 guard 的去重键，两处各写一份迟早漂成两个键。
pub(crate) fn output_file(
    settings: &hongguo_common::model::Settings,
    output_name: &str,
) -> PathBuf {
    settings
        .series_dir(output_name)
        .join(format!("{output_name} 合集.mp4"))
}

/// 可合并的剧列表，按可合并集数倒序。从下载队列聚合而不是读剧集档案：
/// 「档案还在不在」与「能不能合」是两件事（磁盘清理页移除过记录的剧文件和任务都还在，照样该出现）。
/// 剧名取自任务记录里的 `series_title`，档案缺失时也还有名字。
pub fn candidates(state: &AppState) -> Vec<MergeCandidate> {
    let mut by_series: std::collections::HashMap<String, MergeCandidate> =
        std::collections::HashMap::new();

    for task in state.queue().all() {
        if !task.is_done() || !Path::new(&task.file_path).exists() {
            continue;
        }
        let entry = by_series
            .entry(task.series_id.clone())
            .or_insert_with(|| MergeCandidate {
                series_id: task.series_id.clone(),
                series_title: task.series_title.clone(),
                episode_count: 0,
                total_size: 0,
            });
        entry.episode_count += 1;
        entry.total_size += std::fs::metadata(&task.file_path)
            .map(|m| m.len())
            .unwrap_or(0);
    }

    let mut list: Vec<MergeCandidate> = by_series.into_values().collect();
    // 集数多的排前面：用户来这一页通常是要合并下得最全的那部
    list.sort_by(|a, b| {
        b.episode_count
            .cmp(&a.episode_count)
            .then_with(|| a.series_title.cmp(&b.series_title))
    });
    list
}

/// 取消仍在运行的合并后删除其记录（不删已产出的文件）。找不到就报 [`AppError::NotFound`]，
/// 与 `stop_download` / `retry_task` 同一口径：返回 `Ok` 却什么都没删，调用方会以为删掉了。
pub fn remove_task(state: &AppState, id: &str) -> AppResult<()> {
    let task = state
        .store
        .merge_tasks()?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| AppError::NotFound(format!("合并任务 {id}")))?;
    if task.status == hongguo_common::model::MergeStatus::Running
        && guard::cancel(&task.output_name)
    {
        log::info!("[Merge] 已请求取消 {}", task.output_name);
    }
    if state.store.delete_merge_task(id)? == 0 {
        return Err(AppError::NotFound(format!("合并任务 {id}")));
    }
    Ok(())
}
