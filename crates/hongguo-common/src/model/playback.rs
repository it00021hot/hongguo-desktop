//! 播放进度模型（断点续播）。

use serde::{Deserialize, Serialize};

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

/// 未完成的集看过超过此秒数时续播。
pub const RESUME_THRESHOLD_SECS: f64 = 3.0;

pub fn resume_position(current_time: f64, duration: f64) -> f64 {
    if current_time.is_finite()
        && duration.is_finite()
        && current_time > RESUME_THRESHOLD_SECS
        && (duration <= 0.0 || current_time < duration)
    {
        current_time
    } else {
        0.0
    }
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

    /// 已完成的集重新从头播放，未完成的集保留有效断点。
    pub fn is_resumable(&self) -> bool {
        resume_position(self.current_time, self.duration) > 0.0
    }
}

/// 播放请求。由前端 `playRequestSchema` 发出，键名 camelCase。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub series_id: String,
    pub vid_index: u32,
    /// 指定本地文件路径；未指定时由 prefer_online 决定在线或下载记录。
    #[serde(default)]
    pub file_path: String,
    /// 优先在线播放（不落盘）
    #[serde(default)]
    pub prefer_online: bool,
    /// 用户手选清晰度；未指定时按官方记忆、缓存与动态策略选择。
    #[serde(default)]
    pub definition: Option<u32>,
    /// 用户点击自动：清除当前官方作用域的手选记忆。
    #[serde(default)]
    pub reset_definition: bool,
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
    /// `error` 非空时它的种类（[`AppError::i18n_key`](crate::AppError::i18n_key)），否则为空。
    /// 前端按种类取译文，并据此分辨「重试也没用」的失败（如只提供内置解码器解不了的编码）。
    #[serde(default)]
    pub error_kind: String,
    /// 实际生效的清晰度档位
    #[serde(default)]
    pub definition: u32,
    /// 自动策略与手选可能使用同一档流，模式不得从 definition 推断。
    #[serde(default)]
    pub automatic_definition: bool,
    /// 本集提供的全部档位，供切换菜单渲染
    #[serde(default)]
    pub definitions: Vec<VideoDefinition>,
    /// 选中流的编码，本地探测失败时为空。
    #[serde(default)]
    pub codec: String,
    /// 这条流交给内置软解直播放（HEVC / ByteVC1 / ByteVC2），而不是喂给系统播放器。
    #[serde(default)]
    pub direct: bool,
}

/// 一档清晰度，发前端 `videoDefinitionSchema`。派生 `Ord`：取流侧按 (档位, 宽, 高) 升序去重后一次反转即得「高到低」，不必写比较函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDefinition {
    /// 档位数值（1080 / 720 / 540…）
    pub value: u32,
    pub width: u32,
    pub height: u32,
}
