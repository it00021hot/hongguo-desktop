//! 历史域数据模型（一条云端观看记录 / 云端观看历史一页）。

use serde::{Deserialize, Serialize};

/// 一条云端观看记录。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistoryItem {
    pub series_id: String,
    pub title: String,
    /// 封面（HEIC 签名 URL；前端走 hongguo-cover 代理渲染）
    #[serde(default)]
    pub cover: String,
    /// 观看到第几集（**1 起**；2026-10-09 实证：云端行的 vid 与该集 vid
    /// 一一对应——18 集完结剧的行 vid_index=18、vid=第 18 集 vid。
    /// ⚠️ 第三方 hgplayer 上报 0 基，云端两种基混杂：消费方对 0 按
    /// 「第 1 集」钳制）
    pub vid_index: i64,
    /// 那一集的 vid（数字字段，转字符串承载）
    #[serde(default)]
    pub vid: String,
    /// 观看进度（毫秒）
    pub position_ms: i64,
    /// 该集时长（毫秒；0 = 未知）
    #[serde(default)]
    pub duration_ms: i64,
    /// 总集数
    #[serde(default)]
    pub episode_cnt: i64,
    /// 最近观看时间（unix 毫秒）
    pub updated_at_ms: i64,
}

/// 云端观看历史一页。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistoryPage {
    pub items: Vec<WatchHistoryItem>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_offset: i64,
    #[serde(default)]
    pub total: i64,
}
