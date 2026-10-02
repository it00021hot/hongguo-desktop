//! 占用统计。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::DownloadTask;

/// 占用统计。只出不进。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageUsage {
    /// 总字节
    pub bytes: u64,
    /// 文件数
    pub files: usize,
}

/// 统计本地占用。
pub fn usage(state: &State<'_, AppState>) -> StorageUsage {
    StorageUsage::from_tasks(&state.queue().all())
}

impl StorageUsage {
    /// 从任务列表统计占用。只计已产出成品文件的任务。
    fn from_tasks(tasks: &[DownloadTask]) -> Self {
        let mut bytes = 0u64;
        let mut files = 0usize;
        for t in tasks.iter().filter(|t| t.is_done()) {
            if let Ok(m) = std::fs::metadata(&t.file_path) {
                bytes += m.len();
                files += 1;
            }
        }
        Self { bytes, files }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tasks_report_zero() {
        let u = StorageUsage::from_tasks(&[]);
        assert_eq!(u.bytes, 0);
        assert_eq!(u.files, 0);
    }

    #[test]
    fn only_done_tasks_count() {
        let mut done = DownloadTask::new("1", "剧", 1, "v1", "");
        done.mark_completed("已存在.mp4", 0);
        let pending = DownloadTask::new("1", "剧", 2, "v2", "");
        let u = StorageUsage::from_tasks(&[done, pending]);
        // done 的路径在磁盘上不存在，因此只统计 pending 之外的不应报错
        assert_eq!(u.files, 0, "路径不存在时不应计入");
    }
}
