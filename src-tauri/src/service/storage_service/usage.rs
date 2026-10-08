//! 占用统计。

use std::collections::HashMap;

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

/// 按剧聚合的磁盘占用（清理页的剧列表）。
///
/// 剧名与分组都取自任务记录（与合并服务同一来源），不依赖剧集档案——
/// 档案是「看过的剧」的全集，清理页要的是「磁盘上有文件的剧」。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageSeriesUsage {
    pub series_id: String,
    pub title: String,
    /// 实际落盘字节
    pub bytes: u64,
    /// 实际存在的成品文件数
    pub files: usize,
}

/// 统计本地占用。
pub fn usage(state: &State<'_, AppState>) -> StorageUsage {
    StorageUsage::from_tasks(&state.queue().all())
}

/// 按剧聚合磁盘占用。只列磁盘上真有文件的剧（metadata 摸不到的直接跳过，
/// 「记录在、文件已被手动删」的剧不出现，免得列出一堆 0 B 的空行）。
pub fn series_usage(state: &State<'_, AppState>) -> Vec<StorageSeriesUsage> {
    series_from_tasks(&state.queue().all())
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

/// 按任务记录聚合（保持队列顺序）。只有磁盘上摸得到文件的任务参与分组。
fn series_from_tasks(tasks: &[DownloadTask]) -> Vec<StorageSeriesUsage> {
    let mut out: Vec<StorageSeriesUsage> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for t in tasks.iter().filter(|t| t.is_done()) {
        let Ok(m) = std::fs::metadata(&t.file_path) else {
            continue;
        };
        let bytes = m.len();
        match index.get(&t.series_id) {
            Some(&i) => {
                let s = &mut out[i];
                s.bytes += bytes;
                s.files += 1;
            }
            None => {
                index.insert(t.series_id.clone(), out.len());
                out.push(StorageSeriesUsage {
                    series_id: t.series_id.clone(),
                    title: t.series_title.clone(),
                    bytes,
                    files: 1,
                });
            }
        }
    }
    out
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

    #[test]
    fn series_aggregation_groups_by_task_and_skips_missing_files() {
        // metadata 只认磁盘上真实存在的文件，造两个临时文件当成品
        let dir = std::env::temp_dir().join(format!("hongguo-usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("a.mp4");
        let f2 = dir.join("b.mp4");
        std::fs::write(&f1, [0u8; 10]).unwrap();
        std::fs::write(&f2, [0u8; 30]).unwrap();

        let mut done1 = DownloadTask::new("1", "剧一", 1, "v1", "第一集");
        done1.mark_completed(f1.to_str().unwrap(), 10);
        let mut done2 = DownloadTask::new("1", "剧一", 2, "v2", "第二集");
        done2.mark_completed("不存在.mp4", 0);
        let mut done3 = DownloadTask::new("2", "剧二", 1, "v3", "第一集");
        done3.mark_completed(f2.to_str().unwrap(), 30);
        let failed = DownloadTask::new("3", "剧三", 1, "v4", "第一集");

        let list = series_from_tasks(&[done1, done2, done3, failed]);

        // 剧一只有一集摸得到文件；剧三（failed）整部不出现
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].series_id, "1");
        assert_eq!(list[0].title, "剧一");
        assert_eq!(list[0].files, 1);
        assert_eq!(list[0].bytes, 10);
        assert_eq!(list[1].series_id, "2");
        assert_eq!(list[1].bytes, 30);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
