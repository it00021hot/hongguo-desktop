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

/// 一部剧「最近看到的那一集」。发给前端 `seriesProgressSchema`，camelCase。
///
/// 详情页「继续看第 N 集」的真值来源：本地 playback 表 5 秒一写，
/// 比云端观看历史（约 1 分钟一报 + 缓存）新鲜。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesProgress {
    pub vid_index: u32,
    /// 已播放秒数
    pub current_time: f64,
    /// 总时长（秒）
    #[serde(default)]
    pub duration: f64,
    /// 最近更新（毫秒时间戳）
    #[serde(default)]
    pub updated_at: i64,
}

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
    /// 指定清晰度档位；不给则取平台提供的最高档
    #[serde(default)]
    pub definition: Option<u32>,
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
    /// 实际生效的清晰度档位
    #[serde(default)]
    pub definition: u32,
    /// 本集提供的全部档位，供切换菜单渲染
    #[serde(default)]
    pub definitions: Vec<VideoDefinition>,
}

/// 一档清晰度。发给前端 `videoDefinitionSchema`。
///
/// 派生 `Ord` 是为了在取流侧按 (档位, 宽, 高) 升序去重后再反转，
/// 那样「高到低」就是一次反转，不必写比较函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDefinition {
    /// 档位数值（1080 / 720 / 540…）
    pub value: u32,
    pub width: u32,
    pub height: u32,
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
            definition: 1080,
            definitions: vec![VideoDefinition {
                value: 1080,
                width: 1920,
                height: 1080,
            }],
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["url"], "hongguo-stream://v1");
        assert_eq!(v["online"], true);
        assert_eq!(v["resumeAt"], 12.5);
        assert_eq!(v["definition"], 1080);
        assert_eq!(v["definitions"][0]["value"], 1080);
        assert!(v.get("resume_at").is_none());
    }

    #[test]
    fn play_request_accepts_a_definition() {
        // 不带 definition 的旧调用要照常工作：None = 取最高档
        let r: PlayRequest = serde_json::from_str(r#"{"seriesId":"1","vidIndex":2}"#).unwrap();
        assert_eq!(r.definition, None);

        let r: PlayRequest =
            serde_json::from_str(r#"{"seriesId":"1","vidIndex":2,"definition":720}"#).unwrap();
        assert_eq!(r.definition, Some(720));
    }

    #[test]
    fn position_roundtrips() {
        let p = PlaybackPosition::new(12.5, 300.0);
        let json = serde_json::to_string(&p).unwrap();
        let back: PlaybackPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
