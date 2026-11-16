//! 历史域数据模型（一条云端观看记录 / 云端观看历史一页）。

use serde::{Deserialize, Serialize};

use hongguo_common::model::LocalHistoryItem;

/// 一条云端观看记录。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistoryItem {
    pub series_id: String,
    pub title: String,
    /// 封面（HEIC 签名 URL；前端走 hongguo-cover 代理渲染）
    #[serde(default)]
    pub cover: String,
    /// 观看到第几集（**1 起**；⚠️ 早期客户端上报 0 基，云端两种基混杂：
    /// 消费方对 0 按「第 1 集」钳制）
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

impl From<LocalHistoryItem> for WatchHistoryItem {
    fn from(item: LocalHistoryItem) -> Self {
        Self {
            series_id: item.series_id,
            title: item.title,
            cover: item.cover,
            vid_index: i64::from(item.vid_index),
            vid: item.vid,
            position_ms: item.position_ms,
            duration_ms: item.duration_ms,
            episode_cnt: i64::from(item.episode_cnt),
            updated_at_ms: item.updated_at_ms,
        }
    }
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
