//! 播放进度模型（断点续播）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单集的播放位置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaybackPosition {
    /// 已播放秒数
    pub current_time: f64,
    /// 总时长（秒），仅用于展示
    #[serde(default)]
    pub duration: f64,
    /// 最近更新（毫秒时间戳）
    #[serde(default)]
    pub updated_at: i64,
}

/// 全部剧集的播放进度。
pub type PlaybackMap = BTreeMap<String, BTreeMap<u32, PlaybackPosition>>;
impl PlaybackPosition {
    /// 构造位置。
    pub fn new(current_time: f64, duration: f64) -> Self {
        Self {
            current_time,
            duration,
            updated_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// 是否已接近片尾（剩余不足 10 秒或已看完 95%），此时不该续播。
    pub fn is_near_end(&self) -> bool {
        if self.duration > 0.0 && self.current_time / self.duration > 0.95 {
            return true;
        }
        self.duration > 0.0 && (self.duration - self.current_time) < 10.0
    }
}

/// 播放请求。由前端 `playRequestSchema` 发出，键名 camelCase。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub series_id: String,
    pub vid_index: u32,
    /// 指定文件路径；不给则由后端从任务表里找
    #[serde(default)]
    pub file_path: String,
    /// 优先在线播放（不落盘）
    #[serde(default)]
    pub prefer_online: bool,
}

/// 播放地址响应。发给前端 `playResponseSchema`，键名 camelCase。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayResponse {
    /// 可直接喂给 `<video src>` 的地址
    pub url: String,
    /// 是否走了在线播放（不落盘）
    #[serde(default)]
    pub online: bool,
    /// 建议的续播位置（秒）
    #[serde(default)]
    pub resume_at: f64,
    #[serde(default)]
    pub error: String,
}

/// 播放历史的一条：某部剧最近一次看到的位置。
///
/// 列表页要靠它排序并显示「上次看到第 N 集」，否则用户播过的剧只能靠重新搜。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackHistoryItem {
    pub series_id: String,
    /// 最近播放的集号
    pub vid_index: u32,
    /// 播到多少秒
    pub current_time: f64,
    /// 最后一次播放的毫秒时间戳，用于按最近排序
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_records_current_timestamp() {
        // 续播进度按 updated_at 排序，构造时必须打上当前时间戳
        let before = chrono::Utc::now().timestamp_millis();
        let p = PlaybackPosition::new(10.0, 100.0);
        assert!(p.updated_at >= before, "updated_at 应在构造时刷新");
    }

    #[test]
    fn near_end_detection() {
        assert!(PlaybackPosition::new(96.0, 100.0).is_near_end());
        assert!(PlaybackPosition::new(95.0, 100.0).is_near_end());
        assert!(!PlaybackPosition::new(10.0, 100.0).is_near_end());
        // 时长未知时不判定
        assert!(!PlaybackPosition::new(999.0, 0.0).is_near_end());
    }

    #[test]
    fn play_request_defaults() {
        let r: PlayRequest = serde_json::from_str(r#"{"seriesId":"1","vidIndex":2}"#).unwrap();
        assert!(!r.prefer_online);
        assert!(r.file_path.is_empty());
    }

    #[test]
    fn play_request_reads_frontend_camel_case() {
        // 前端 playRequestSchema 发出的是 camelCase；之前这里是 snake_case，
        // 反序列化直接 missing field，play_series 根本进不来
        let r: PlayRequest = serde_json::from_str(
            r#"{"seriesId":"7687919221593885758","vidIndex":3,"filePath":"","preferOnline":true}"#,
        )
        .expect("camelCase 载荷应能反序列化");
        assert_eq!(r.series_id, "7687919221593885758");
        assert_eq!(r.vid_index, 3);
        assert!(r.prefer_online);
    }

    #[test]
    fn play_response_goes_out_as_camel_case() {
        let r = PlayResponse {
            url: "hongguo-stream://v1".into(),
            online: true,
            resume_at: 12.5,
            error: String::new(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["url"], "hongguo-stream://v1");
        assert_eq!(v["online"], true);
        assert_eq!(v["resumeAt"], 12.5);
        assert!(v.get("resume_at").is_none());
    }

    #[test]
    fn position_roundtrips() {
        let p = PlaybackPosition::new(12.5, 300.0);
        let json = serde_json::to_string(&p).unwrap();
        let back: PlaybackPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
