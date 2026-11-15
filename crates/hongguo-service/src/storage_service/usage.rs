//! 占用统计。

use std::collections::HashMap;

use crate::app_state::AppState;
use hongguo_common::model::DownloadTask;

/// 占用统计。只出不进。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// 总字节
    pub bytes: u64,
    /// 文件数
    pub files: usize,
}

/// 按剧聚合的磁盘占用（清理页的剧列表）。剧名与分组都取自任务记录（与合并服务同一来源），
/// 不依赖剧集档案——清理页要的是「磁盘上有文件的剧」。
/// serde camelCase 对齐前端 schema（seriesId），snake_case 会被 zod 校验整单拒收。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSeriesUsage {
    pub series_id: String,
    pub title: String,
    /// 实际落盘字节
    pub bytes: u64,
    /// 实际存在的成品文件数
    pub files: usize,
}

/// 统计本地占用。
pub fn usage(state: &AppState) -> StorageUsage {
    StorageUsage::from_tasks(&state.queue().all())
}

/// 按剧聚合磁盘占用。只列磁盘上真有文件的剧（metadata 摸不到的直接跳过，
/// 「记录在、文件已被手动删」的剧不出现，免得列出一堆 0 B 的空行）。
pub fn series_usage(state: &AppState) -> Vec<StorageSeriesUsage> {
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
