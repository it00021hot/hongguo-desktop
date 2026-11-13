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

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份各字段都「正常」的校验结果。用例只关心自己断言的那一两个字段。
    fn preflight() -> MergePreflight {
        MergePreflight {
            ok: true,
            episode_count: 7,
            estimated_size: 100,
            free_space: Some(1000),
            codec_consistent: true,
            codec_mismatch_episode: None,
            warnings: vec![],
        }
    }

    #[test]
    fn new_merge_task_is_pending() {
        let t = MergeTask::new("123", "剧名", "剧名 合集", MergeMode::Quick);
        assert_eq!(t.status, MergeStatus::Pending);
        assert_eq!(t.mode, MergeMode::Quick);
        assert_eq!(t.percent, 0.0);
    }

    #[test]
    fn mark_completed_sets_full_progress() {
        let mut t = MergeTask::new("1", "t", "out", MergeMode::Compat);
        t.mark_completed("out.mp4", 999);
        assert_eq!(t.status, MergeStatus::Completed);
        assert_eq!(t.percent, 100.0);
        assert_eq!(t.output_size, 999);
    }

    #[test]
    fn merge_mode_serializes_in_camel_case() {
        let json = serde_json::to_string(&MergeMode::Quick).unwrap();
        assert_eq!(json, "\"quick\"");
        let json = serde_json::to_string(&MergeMode::Compat).unwrap();
        assert_eq!(json, "\"compat\"");
    }

    #[test]
    fn merge_task_roundtrips() {
        let t = MergeTask::new("1", "剧", "out", MergeMode::Compat);
        let json = serde_json::to_string(&t).unwrap();
        let back: MergeTask = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn merge_task_goes_out_as_camel_case() {
        let t = MergeTask::new("1", "剧", "out", MergeMode::Quick);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["seriesId"], "1");
        assert_eq!(v["outputName"], "out");
        assert_eq!(v["episodeCount"], 0);
        assert!(v.get("series_id").is_none());
    }

    #[test]
    fn preflight_goes_out_as_camel_case() {
        let v = serde_json::to_value(preflight()).unwrap();
        assert_eq!(v["episodeCount"], 7);
        assert_eq!(v["estimatedSize"], 100);
        assert_eq!(v["freeSpace"], 1000);
        assert_eq!(v["codecConsistent"], true);
        assert_eq!(v["codecMismatchEpisode"], serde_json::Value::Null);
        assert!(v.get("episode_count").is_none());
        assert!(v.get("codec_consistent").is_none());
    }

    #[test]
    fn preflight_reports_unknown_free_space_as_null() {
        let v = serde_json::to_value(MergePreflight {
            free_space: None,
            ..preflight()
        })
        .unwrap();
        assert!(v["freeSpace"].is_null());
    }

    #[test]
    fn preflight_carries_i18n_keys_not_displayed_text() {
        let p = MergePreflight {
            ok: false,
            episode_count: 0,
            estimated_size: 0,
            free_space: None,
            codec_consistent: true,
            codec_mismatch_episode: None,
            warnings: vec![
                "merge.noDownloads".to_string(),
                "merge.codecMismatch".to_string(),
            ],
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["warnings"][0], "merge.noDownloads");
        assert_eq!(v["warnings"][1], "merge.codecMismatch");
    }

    #[test]
    fn preflight_field_set_matches_the_frontend_contract() {
        let v = serde_json::to_value(preflight()).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "codecConsistent",
                "codecMismatchEpisode",
                "episodeCount",
                "estimatedSize",
                "freeSpace",
                "ok",
                "warnings"
            ]
        );
    }

    #[test]
    fn preflight_reports_the_mismatched_episode() {
        let p = MergePreflight {
            codec_consistent: false,
            codec_mismatch_episode: Some(2),
            ..preflight()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["codecConsistent"], false);
        assert_eq!(v["codecMismatchEpisode"], 2);
    }
}
