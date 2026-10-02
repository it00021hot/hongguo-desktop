//! 下载队列状态机。
//!
//! 只管任务集合与状态流转；并发由 [`super::scheduler`] 控制，
//! 单集执行在 [`super::worker`]，事件发射在 [`super::events`]。
//!
//! 拆成三个文件，避免这个文件长成新的 main.js：
//! - 本文件：状态流转（写路径）——改动频繁，要跟调度器对齐
//! - `queue_query.rs`：只读聚合（读路径）——只被 UI 读，改动少
//! - `queue_tests.rs`：单元测试

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;

#[path = "queue_query.rs"]
mod query;

use parking_lot::RwLock;

use crate::domain::model::{DownloadTask, TaskStatus};

/// 下载队列。
pub struct DownloadQueue {
    tasks: RwLock<Vec<DownloadTask>>,
    /// 正在运行的任务数
    active: RwLock<usize>,
    /// 并发上限
    limit: RwLock<usize>,
}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadQueue {
    /// 构造空队列（并发上限默认 3）。
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(Vec::new()),
            active: RwLock::new(0),
            limit: RwLock::new(3),
        }
    }

    /// 从持久化数据恢复。
    pub fn restore(tasks: Vec<DownloadTask>, limit: usize) -> Self {
        // 启动时把「运行中」视为中断，改回待运行以便续跑
        let restored: Vec<DownloadTask> = tasks
            .into_iter()
            .map(|mut t| {
                if t.status == TaskStatus::Running {
                    t.status = TaskStatus::Pending;
                }
                t
            })
            .collect();
        Self {
            tasks: RwLock::new(restored),
            active: RwLock::new(0),
            limit: RwLock::new(limit.clamp(1, 10)),
        }
    }

    /// 入队，返回新任务。
    pub fn enqueue(&self, task: DownloadTask) -> DownloadTask {
        let mut guard = self.tasks.write();
        // 同一集已在队列里就不重复入队
        if let Some(existing) = guard
            .iter()
            .find(|t| t.series_id == task.series_id && t.vid_index == task.vid_index)
        {
            return existing.clone();
        }
        guard.push(task.clone());
        task
    }

    /// 全部任务快照。
    pub fn all(&self) -> Vec<DownloadTask> {
        self.tasks.read().clone()
    }

    /// 按 id 取任务。
    pub fn get(&self, id: &str) -> Option<DownloadTask> {
        self.tasks.read().iter().find(|t| t.id == id).cloned()
    }

    /// 某剧的任务。
    pub fn of_series(&self, series_id: &str) -> Vec<DownloadTask> {
        self.tasks
            .read()
            .iter()
            .filter(|t| t.series_id == series_id)
            .cloned()
            .collect()
    }

    /// 取出下一个可运行的任务（不修改状态）。
    pub fn next_runnable(&self) -> Option<DownloadTask> {
        self.tasks
            .read()
            .iter()
            .find(|t| t.status.is_runnable())
            .cloned()
    }

    /// 标记为运行中，返回是否成功（false 表示状态已变）。
    pub fn mark_running(&self, id: &str) -> bool {
        let mut guard = self.tasks.write();
        match guard.iter_mut().find(|t| t.id == id) {
            Some(t) if t.status.is_runnable() => {
                t.status = TaskStatus::Running;
                t.error.clear();
                t.touch();
                *self.active.write() += 1;
                true
            }
            _ => false,
        }
    }

    /// 更新下载进度。
    pub fn update_progress(&self, id: &str, downloaded: u64, total: u64) {
        let mut guard = self.tasks.write();
        if let Some(t) = guard.iter_mut().find(|t| t.id == id) {
            t.downloaded = downloaded;
            if total > 0 {
                t.total = total;
            }
            t.touch();
        }
    }

    /// 标记完成。
    pub fn mark_completed(&self, id: &str, path: &str, size: u64) {
        let mut guard = self.tasks.write();
        if let Some(t) = guard.iter_mut().find(|t| t.id == id) {
            t.mark_completed(path, size);
            self.release_active();
        }
    }

    /// 标记失败。
    pub fn mark_failed(&self, id: &str, reason: &str) {
        let mut guard = self.tasks.write();
        if let Some(t) = guard.iter_mut().find(|t| t.id == id) {
            t.mark_failed(reason);
            self.release_active();
        }
    }

    /// 标记停止。
    pub fn mark_stopped(&self, id: &str) {
        let mut guard = self.tasks.write();
        if let Some(t) = guard.iter_mut().find(|t| t.id == id) {
            t.mark_stopped();
            self.release_active();
        }
    }

    /// 重置为待运行，返回被重置的任务。
    pub fn retry(&self, id: &str) -> Option<DownloadTask> {
        let mut guard = self.tasks.write();
        let t = guard.iter_mut().find(|t| t.id == id)?;
        t.reset_for_retry();
        Some(t.clone())
    }

    /// 批量重置。
    pub fn retry_many(&self, ids: &[String]) -> usize {
        let mut guard = self.tasks.write();
        let mut n = 0;
        for t in guard.iter_mut() {
            if ids.contains(&t.id) {
                t.reset_for_retry();
                n += 1;
            }
        }
        n
    }

    /// 删除任务。
    pub fn remove(&self, ids: &[String]) -> usize {
        let mut guard = self.tasks.write();
        let before = guard.len();
        guard.retain(|t| !ids.contains(&t.id));
        before - guard.len()
    }

    /// 摘掉某剧某集的任务记录（本地文件已被清掉时用）。
    ///
    /// 记录留着比文件没了更糟：播放按「已完成」判定走本地协议去开那个文件，
    /// 而文件已经不在，于是直接变成「媒体处理失败」，连回落在线流的机会都没有。
    pub fn remove_episode(&self, series_id: &str, vid_index: u32) -> usize {
        let mut guard = self.tasks.write();
        let before = guard.len();
        guard.retain(|t| !(t.series_id == series_id && t.vid_index == vid_index));
        before - guard.len()
    }

    /// 一键暂停：取消进行中的任务并清空待运行队列。
    pub fn pause_all(&self) -> usize {
        let mut guard = self.tasks.write();
        let mut n = 0;
        for t in guard.iter_mut() {
            if matches!(t.status, TaskStatus::Pending | TaskStatus::Running) {
                t.mark_stopped();
                n += 1;
            }
        }
        *self.active.write() = 0;
        n
    }

    /// 一键启动：把未完成任务重新排队。
    pub fn resume_all(&self) -> usize {
        let mut guard = self.tasks.write();
        let mut n = 0;
        for t in guard.iter_mut() {
            if matches!(t.status, TaskStatus::Stopped | TaskStatus::Failed) {
                t.reset_for_retry();
                n += 1;
            }
        }
        n
    }

    /// 设置并发上限，保存后立即生效。
    pub fn set_limit(&self, limit: usize) {
        *self.limit.write() = limit.clamp(1, 10);
    }

    /// 当前并发上限。
    pub fn limit(&self) -> usize {
        *self.limit.read()
    }

    /// 归还一个运行名额（任务收尾时调用）。
    ///
    /// 用 `saturating_sub` 而非硬减：`pause_all` 会把计数直接清零，
    /// 而在途任务要等下一轮才回来报告收尾，硬减会在 usize 上 panic。
    /// 读锁必须在写锁之前放开，否则 `parking_lot` 会自己等死。
    fn release_active(&self) {
        let next = self.active.read().saturating_sub(1);
        *self.active.write() = next;
    }
}
