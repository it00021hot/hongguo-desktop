//! 观看历史模型：本地行与云端条目共用同一字段形态（历史页两种模式一张卡）。

use serde::{Deserialize, Serialize};

/// 本地观看历史的一行（issue #10：未登录落本地可记可管，登录后补传云端）。字段与云端 `history::WatchHistoryItem` 一一同名（camelCase），历史页两种模式共用同一张卡片。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHistoryItem {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    /// 观看到第几集（1 起；与云端条目同口径，0 基旧数据钳到 1 展示）
    pub vid_index: u32,
    pub vid: String,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub episode_cnt: u32,
    pub updated_at_ms: i64,
}
