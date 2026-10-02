//! 下载任务模型。

use serde::{Deserialize, Serialize};

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    /// 等待中（已入队未开始）
    Pending,
    /// 下载中
    Running,
    /// 已完成
    Completed,
    /// 失败，可重试
    Failed,
    /// 用户主动停止
    Stopped,
}

impl TaskStatus {
    /// 调度器是否应当挑起这个任务。
    pub fn is_runnable(&self) -> bool {
        matches!(self, TaskStatus::Pending | TaskStatus::Failed)
    }
}

/// 一个下载任务（对应一集）。
///
/// 落盘沿用 Electron 版的 snake_case 键名（见 [`crate::domain::model::series`]），
/// 发给前端时转 camelCase（前端 `downloadTaskSchema`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    #[serde(alias = "series_id")]
    pub series_id: String,
    #[serde(alias = "series_title")]
    pub series_title: String,
    #[serde(alias = "vid_index")]
    pub vid_index: u32,
    pub vid: String,
    #[serde(default, alias = "ep_title")]
    pub ep_title: String,
    /// 成品路径
    #[serde(default, alias = "file_path")]
    pub file_path: String,
    /// 下载中的临时路径（`.enc.tmp`），解密成功后才改名
    #[serde(default, alias = "temp_path")]
    pub temp_path: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub error: String,
    #[serde(default, alias = "created_at")]
    pub created_at: i64,
    #[serde(default, alias = "updated_at")]
    pub updated_at: i64,
}

impl DownloadTask {
    /// 构造新任务。
    pub fn new(
        series_id: &str,
        series_title: &str,
        vid_index: u32,
        vid: &str,
        ep_title: &str,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            series_id: series_id.to_string(),
            series_title: series_title.to_string(),
            vid_index,
            vid: vid.to_string(),
            ep_title: ep_title.to_string(),
            file_path: String::new(),
            temp_path: String::new(),
            status: TaskStatus::Pending,
            downloaded: 0,
            total: 0,
            error: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// 进度百分比（0.0–100.0），总大小未知时返回 0。
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        ((self.downloaded as f64 / self.total as f64) * 100.0).clamp(0.0, 100.0)
    }

    /// 是否已产出可播放文件。
    pub fn is_done(&self) -> bool {
        self.status == TaskStatus::Completed && !self.file_path.is_empty()
    }

    /// 标记完成。
    pub fn mark_completed(&mut self, path: &str, size: u64) {
        self.status = TaskStatus::Completed;
        self.file_path = path.to_string();
        self.downloaded = size;
        self.total = size;
        self.error.clear();
        self.temp_path.clear();
        self.touch();
    }

    /// 标记失败。
    pub fn mark_failed(&mut self, reason: &str) {
        self.status = TaskStatus::Failed;
        self.error = reason.to_string();
        self.touch();
    }

    /// 标记停止。
    pub fn mark_stopped(&mut self) {
        self.status = TaskStatus::Stopped;
        self.touch();
    }

    /// 重置为待运行（重试用）。
    pub fn reset_for_retry(&mut self) {
        self.status = TaskStatus::Pending;
        self.error.clear();
        self.downloaded = 0;
        self.touch();
    }

    /// 刷新时间戳。
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().timestamp_millis();
    }
}

/// 队列状态概览。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueStatus {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    /// 当前并发上限
    pub limit: usize,
    /// 正在并发下载的数量
    pub active: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_is_pending() {
        let t = DownloadTask::new("123", "剧名", 3, "v3", "第三集");
        assert_eq!(t.status, TaskStatus::Pending);
        assert_eq!(t.percent(), 0.0);
        assert!(!t.id.is_empty());
    }

    #[test]
    fn percent_is_clamped() {
        let mut t = DownloadTask::new("1", "t", 1, "v", "");
        t.downloaded = 50;
        t.total = 100;
        assert_eq!(t.percent(), 50.0);
        t.downloaded = 200;
        assert_eq!(t.percent(), 100.0);
    }

    #[test]
    fn percent_is_zero_when_size_unknown() {
        let mut t = DownloadTask::new("1", "t", 1, "v", "");
        t.total = 0;
        t.downloaded = 999;
        assert_eq!(t.percent(), 0.0);
    }

    #[test]
    fn completed_task_reports_full_percent() {
        // 调度器的进度事件直接复用这个口径，完成态必须报到 100
        let mut t = DownloadTask::new("1", "t", 1, "v", "");
        t.mark_completed("a.mp4", 100);
        assert_eq!(t.percent(), 100.0);
    }

    #[test]
    fn mark_completed_sets_path() {
        let mut t = DownloadTask::new("1", "t", 1, "v", "");
        t.temp_path = "a.enc.tmp".into();
        t.mark_completed("a.mp4", 1234);
        assert!(t.is_done());
        assert_eq!(t.file_path, "a.mp4");
        assert!(t.temp_path.is_empty());
    }

    #[test]
    fn retry_clears_error_and_progress() {
        let mut t = DownloadTask::new("1", "t", 1, "v", "");
        t.mark_failed("网络断了");
        assert_eq!(t.status, TaskStatus::Failed);
        t.reset_for_retry();
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(t.error.is_empty());
        assert_eq!(t.downloaded, 0);
    }

    #[test]
    fn status_predicates() {
        assert!(TaskStatus::Pending.is_runnable());
        assert!(TaskStatus::Failed.is_runnable());
        assert!(!TaskStatus::Running.is_runnable());
        assert!(!TaskStatus::Completed.is_runnable());
        assert!(!TaskStatus::Stopped.is_runnable());
    }

    #[test]
    fn task_roundtrips_through_json() {
        let t = DownloadTask::new("1", "剧", 2, "v2", "第二集");
        let json = serde_json::to_string(&t).unwrap();
        let back: DownloadTask = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn task_reads_legacy_snake_case_data_file() {
        let json = r#"{"id":"a","series_id":"1","series_title":"剧","vid_index":3,"vid":"v3","ep_title":"第三集","file_path":"a.mp4","temp_path":"a.tmp","status":"completed","downloaded":10,"total":10,"error":"","created_at":1,"updated_at":2}"#;
        let t: DownloadTask = serde_json::from_str(json).unwrap();
        assert_eq!(t.series_id, "1");
        assert_eq!(t.vid_index, 3);
        assert_eq!(t.ep_title, "第三集");
        assert_eq!(t.file_path, "a.mp4");
        assert_eq!(t.status, TaskStatus::Completed);
    }

    #[test]
    fn task_goes_out_as_camel_case_for_ipc() {
        // 前端 downloadTaskSchema 声明的是 camelCase
        let t = DownloadTask::new("1", "剧", 2, "v2", "第二集");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["seriesId"], "1");
        assert_eq!(v["seriesTitle"], "剧");
        assert_eq!(v["vidIndex"], 2);
        assert_eq!(v["epTitle"], "第二集");
        assert_eq!(v["filePath"], "");
        assert_eq!(v["tempPath"], "");
        assert!(v.get("created_at").is_none());
        assert!(v.get("updated_at").is_none());
    }
}
