//! 下载队列的只读聚合。
//!
//! 与 `queue.rs` 的分工：那边管状态流转（写），这里只提供查询（读）。
//! 两者都挂在同一个 `impl DownloadQueue` 上，因此方法可以直接互相调用。

use super::DownloadQueue;
use crate::domain::model::{DownloadTask, QueueStatus, TaskStatus};

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

    /// 把某剧的磁盘文件补登记为任务（任务记录丢失时自愈）。
    /// 补登记：磁盘上有文件但任务记录丢了（换机、data.json 损坏）。
    pub fn adopt_file(&self, task: DownloadTask, size: u64) {
        let mut guard = self.tasks.write();
        if let Some(existing) = guard
            .iter_mut()
            .find(|t| t.series_id == task.series_id && t.vid_index == task.vid_index)
        {
            if !existing.is_done() {
                existing.mark_completed(&task.file_path, size);
            }
        } else {
            let mut t = task;
            t.mark_completed(&t.file_path.clone(), size);
            guard.push(t);
        }
    }
}
