//! 下载队列的只读聚合：与 `queue.rs`（写路径）分工，两者挂在同一个
//! `impl DownloadQueue` 上，方法可以直接互相调用。

use super::DownloadQueue;
use hongguo_common::model::{QueueStatus, TaskStatus};

impl DownloadQueue {
    /// 队列状态汇总。
    pub fn status(&self) -> QueueStatus {
        let tasks = self.tasks.read();
        let mut s = QueueStatus {
            limit: *self.limit.read(),
            active: *self.active.read(),
            ..Default::default()
        };
        for t in tasks.iter() {
            match t.status {
                TaskStatus::Pending => s.pending += 1,
                TaskStatus::Running => s.running += 1,
                TaskStatus::Completed => s.completed += 1,
                TaskStatus::Failed => s.failed += 1,
                TaskStatus::Stopped => {}
            }
        }
        s
    }

    /// 某剧某集已完成任务的成品路径。
    pub fn completed_path(&self, series_id: &str, vid_index: u32) -> Option<String> {
        self.tasks
            .read()
            .iter()
            .find(|t| t.series_id == series_id && t.vid_index == vid_index && t.is_done())
            .map(|t| t.file_path.clone())
    }

    /// 某剧已下载完成的集号（选集完成角标数据源）。
    pub fn completed_vids(&self, series_id: &str) -> Vec<u32> {
        self.tasks
            .read()
            .iter()
            .filter(|t| t.series_id == series_id && t.is_done())
            .map(|t| t.vid_index)
            .collect()
    }
}
