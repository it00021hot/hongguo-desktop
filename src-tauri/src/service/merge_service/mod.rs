//! 合并服务：快速合并（流复制）与兼容合并（转码）。
//!
//! 三条路径（前置校验、快速合并、兼容合并）都要先回答同一个问题：
//! 「哪些集真的可以参与合并」。答案统一由 [`done_inputs`] 给出，
//! 三处各写一遍时迟早会漂移成三种口径。

pub mod compat;
pub mod guard;
pub mod prepare;
pub mod progress;
pub mod quick;

use std::path::{Path, PathBuf};

use crate::app_state::AppState;
use crate::domain::model::MergeCandidate;
use crate::error::{AppError, AppResult};

/// 某剧可参与合并的分集，按集号升序。
///
/// 「已完成」与「文件存在」必须同时成立：用户在系统里删过文件时任务记录
/// 仍留着 completed，只看状态就会把一个不存在的路径交给拼接器。
pub fn done_inputs(state: &AppState, series_id: &str) -> Vec<(u32, PathBuf)> {
    crate::media::remux::sort_by_index(
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

/// 可合并的剧列表，按可合并集数倒序。
///
/// 从下载队列聚合而不是读剧集档案：合并的输入是本地分集文件，
/// 「档案还在不在」与「能不能合」是两件事。用户在磁盘清理页移除过记录的剧，
/// 文件和任务都还在，照样该出现在这里；反过来一部集都没下的剧
/// 出现在下拉里，选中后只会得到一句「没有已下载的分集」。
///
/// 剧名取自任务记录里的 `series_title`（提交下载时从档案抄的），
/// 档案缺失时也还有名字，不会退化成一条空白的候选项。
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

/// 删除一条合并任务记录（只删记录，不删已产出的文件）。
///
/// 找不到就报 [`AppError::NotFound`]，与 `stop_download` / `retry_task` 同一口径：
/// 返回 `Ok` 却什么都没删，调用方会以为删掉了。
pub fn remove_task(state: &AppState, id: &str) -> AppResult<()> {
    let mut data = state.store.write();
    let before = data.merge_tasks.len();
    data.merge_tasks.retain(|t| t.id != id);
    if data.merge_tasks.len() == before {
        return Err(AppError::NotFound(format!("合并任务 {id}")));
    }
    data.save(&crate::store::paths::data_file())
        .map_err(|e| AppError::StoreCorrupt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{DownloadTask, MergeMode, MergeTask};

    /// 造一个指向真实临时文件、状态为已完成的分集。
    fn done_task(
        dir: &std::path::Path,
        series_id: &str,
        vid_index: u32,
        name: &str,
    ) -> DownloadTask {
        let path = dir.join(name);
        std::fs::write(&path, b"x").unwrap();
        let mut t = DownloadTask::new(series_id, "剧", vid_index, "v", "");
        t.mark_completed(&path.to_string_lossy(), 1);
        t
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-merge-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn done_inputs_are_sorted_by_episode_index() {
        let dir = temp_dir("sort");
        let state = AppState::default();
        for idx in [3, 1, 2] {
            let name = format!("{idx}.mp4");
            state.queue().enqueue(done_task(&dir, "1", idx, &name));
        }

        let got = done_inputs(&state, "1");
        assert_eq!(
            got.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "必须按集号数字排序，字符串序会把第 10 集排到第 2 集前面"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn done_inputs_skip_unfinished_and_vanished_files() {
        let dir = temp_dir("skip");
        let state = AppState::default();

        // 已完成且文件在
        state.queue().enqueue(done_task(&dir, "1", 1, "keep.mp4"));
        // 别的剧的已完成分集不参与本剧合并
        state.queue().enqueue(done_task(&dir, "2", 9, "other.mp4"));
        // 已完成但文件已被用户在系统里删掉
        state.queue().enqueue(done_task(&dir, "1", 2, "gone.mp4"));
        std::fs::remove_file(dir.join("gone.mp4")).unwrap();
        // 未完成
        state
            .queue()
            .enqueue(DownloadTask::new("1", "剧", 3, "v", ""));

        let got = done_inputs(&state, "1");
        assert_eq!(
            got.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![1],
            "别的剧、文件已消失、未完成的集都不该进来"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 造一部已下载多集的剧，返回入队后的任务列表已就绪。
    fn enqueue_series(
        state: &AppState,
        dir: &std::path::Path,
        series_id: &str,
        title: &str,
        n: u32,
    ) {
        for idx in 1..=n {
            let mut t = DownloadTask::new(series_id, title, idx, "v", "");
            let path = dir.join(format!("{series_id}-{idx}.mp4"));
            std::fs::write(&path, vec![b'x'; 10]).unwrap();
            t.mark_completed(&path.to_string_lossy(), 10);
            state.queue().enqueue(t);
        }
    }

    #[test]
    fn candidates_come_from_the_queue_not_the_series_registry() {
        let dir = temp_dir("cand-registry");
        let state = AppState::default();
        enqueue_series(&state, &dir, "1", "下过的剧", 3);
        // 档案被软删除（磁盘清理页「移除记录」）：文件和任务都还在，照样合得起来
        state
            .store
            .write()
            .series
            .push(crate::domain::model::Series {
                series_id: "1".into(),
                title: "下过的剧".into(),
                dismissed: true,
                ..Default::default()
            });
        // 档案在但一集没下：不该出现在候选里
        state
            .store
            .write()
            .series
            .push(crate::domain::model::Series {
                series_id: "2".into(),
                title: "没下过的剧".into(),
                ..Default::default()
            });

        let got = candidates(&state);
        assert_eq!(got.len(), 1, "只有真下载过的剧算候选: {got:?}");
        assert_eq!(got[0].series_id, "1");
        assert_eq!(
            got[0].series_title, "下过的剧",
            "剧名取自任务记录，不依赖档案"
        );
        assert_eq!(got[0].episode_count, 3);
        assert_eq!(got[0].total_size, 30);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn candidates_skip_missing_files_and_sort_by_episode_count() {
        let dir = temp_dir("cand-sort");
        let state = AppState::default();
        enqueue_series(&state, &dir, "1", "少", 1);
        enqueue_series(&state, &dir, "2", "多", 5);

        // 已完成但文件被用户在系统里删了：不能算进去
        let mut orphan = DownloadTask::new("3", "孤儿", 1, "v", "");
        orphan.mark_completed(&dir.join("orphan.mp4").to_string_lossy(), 10);
        state.queue().enqueue(orphan);

        let got = candidates(&state);
        assert_eq!(
            got.iter().map(|c| c.series_id.as_str()).collect::<Vec<_>>(),
            ["2", "1"],
            "集数多的排前面；文件已消失的剧不进候选"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn candidates_are_empty_without_downloads() {
        let state = AppState::default();
        state
            .queue()
            .enqueue(DownloadTask::new("1", "剧", 1, "v", ""));
        assert!(candidates(&state).is_empty(), "没下载过就没有候选");
    }

    #[test]
    fn remove_task_drops_only_that_record_and_persists() {
        let dir = temp_dir("rmtask");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        let a = MergeTask::new("1", "剧", "out-a", MergeMode::Quick);
        let b = MergeTask::new("2", "剧", "out-b", MergeMode::Quick);
        state
            .store
            .write()
            .merge_tasks
            .extend([a.clone(), b.clone()]);

        remove_task(&state, &a.id).expect("存在的记录应当删得掉");

        // get_merge_tasks 读的就是这份列表
        let left = state.store.read().merge_tasks.clone();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, b.id, "只删指定的那条");

        let on_disk = crate::store::DataStore::load(&crate::store::paths::data_file());
        assert_eq!(
            on_disk.merge_tasks.len(),
            1,
            "删了要落盘，否则重启记录又回来"
        );
        assert_eq!(on_disk.merge_tasks[0].id, b.id);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_task_reports_not_found_for_unknown_id() {
        let dir = temp_dir("rmtask-missing");
        let _scoped = crate::store::paths::ScopedDataDir::new(&dir);

        let state = AppState::default();
        let kept = MergeTask::new("1", "剧", "out", MergeMode::Quick);
        state.store.write().merge_tasks.push(kept.clone());

        let err = remove_task(&state, "no-such-id").expect_err("不存在的记录必须报错");
        assert!(
            matches!(err, AppError::NotFound(_)),
            "应与 stop_download 一样报 NotFound，实际: {err:?}"
        );
        assert_eq!(err.i18n_key(), "error.notFound");
        assert_eq!(
            state.store.read().merge_tasks.len(),
            1,
            "没删掉任何东西时不该动列表"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
