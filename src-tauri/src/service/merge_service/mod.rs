//! 合并服务：快速合并（流复制）与兼容合并（转码）。
//!
//! 三条路径（前置校验、快速合并、兼容合并）都要先回答同一个问题：
//! 「哪些集真的可以参与合并」。答案统一由 [`done_inputs`] 给出，
//! 三处各写一遍时迟早会漂移成三种口径。

pub mod compat;
pub mod prepare;
pub mod progress;
pub mod quick;

use std::path::PathBuf;

use crate::app_state::AppState;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::DownloadTask;

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
}
