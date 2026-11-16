//! 合并任务模型。

use serde::{Deserialize, Serialize};

/// 合并模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeMode {
    /// 快速合并：流复制，不重编码（无损且极快）
    #[default]
    Quick,
    /// 统一编码合并：HEVC/AAC。
    Compat,
}

/// 合并任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// 一个合并任务。发给前端转 camelCase（前端 schema 同步声明）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeTask {
    pub id: String,
    pub series_id: String,
    pub series_title: String,
    pub output_name: String,
    pub mode: MergeMode,
    pub status: MergeStatus,
    /// 参与合并的集数（按集号升序）
    #[serde(default)]
    pub episode_count: usize,
    /// 输出路径
    #[serde(default)]
    pub output_path: String,
    /// 输出大小
    #[serde(default)]
    pub output_size: u64,
    /// 进度 0.0–100.0
    #[serde(default)]
    pub percent: f64,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub created_at: i64,
}

impl MergeTask {
    /// 构造新任务。
    pub fn new(series_id: &str, series_title: &str, output_name: &str, mode: MergeMode) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            series_id: series_id.to_string(),
            series_title: series_title.to_string(),
            output_name: output_name.to_string(),
            mode,
            status: MergeStatus::Pending,
            episode_count: 0,
            output_path: String::new(),
            output_size: 0,
            percent: 0.0,
            error: String::new(),
            created_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// 标记完成。
    pub fn mark_completed(&mut self, path: &str, size: u64) {
        self.status = MergeStatus::Completed;
        self.output_path = path.to_string();
        self.output_size = size;
        self.percent = 100.0;
        self.error.clear();
    }

    /// 标记失败。
    pub fn mark_failed(&mut self, reason: &str) {
        self.status = MergeStatus::Failed;
        self.error = reason.to_string();
    }
}

/// 一部可合并的剧（合并页下拉列表）。候选来自**下载队列**而非剧集档案：合并真正吃的是队列里已完成、文件仍在的分集（判定见 `service::merge_service::done_inputs`）——档案可被 dismiss 软删除，但本地文件与任务记录还在，那部剧照样合得起来。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCandidate {
    pub series_id: String,
    pub series_title: String,
    /// 可参与合并的集数（已完成且文件仍在）
    pub episode_count: usize,
    /// 已下载分集的字节数之和
    pub total_size: u64,
}

/// 合并前校验结果。只走 IPC，不落盘。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePreflight {
    pub ok: bool,
    /// 参与合并的集数
    pub episode_count: usize,
    /// 预计输出大小
    pub estimated_size: u64,
    /// 输出目录所在卷的剩余字节。`None` = 查不到，不是 0。
    pub free_space: Option<u64>,
    /// 各集编码是否完全一致。快速合并的前提。
    pub codec_consistent: bool,
    /// 第一个与第 1 集编码不一致的集号
    pub codec_mismatch_episode: Option<u32>,
    /// 提示文案。放的是 i18n key，由前端查资源表翻成用户看得懂的文案
    #[serde(default)]
    pub warnings: Vec<String>,
}
