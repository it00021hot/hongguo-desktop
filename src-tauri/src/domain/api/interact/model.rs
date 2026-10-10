//! 互动域数据模型（书架条目 / 互动列表条目 / 互动状态回显）。

use serde::{Deserialize, Serialize};

/// 书架（收藏）列表里的一条。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookshelfEntry {
    pub series_id: String,
    /// 收藏时间（unix 毫秒；0 = 服务端未给）
    #[serde(default)]
    pub collect_time_ms: i64,
    /// 内容类型（1=真人，1004=漫剧；推荐流口味统计用）
    #[serde(default)]
    pub content_type: i64,
}

/// 互动列表里的一条视频（含互动**计数**，右侧栏数字直接用）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionItem {
    pub vid: String,
    pub series_id: String,
    pub user_digg: bool,
    #[serde(default)]
    pub digged_count: i64,
    #[serde(default)]
    pub followed: bool,
    /// 追剧数（hgplayer 右栏 ☆ 下的 21.1万）
    #[serde(default)]
    pub followed_cnt: i64,
    /// 剧标题（video_detail.series_title，「我的点赞」列表页直接展示）
    #[serde(default)]
    pub series_title: String,
}

/// best-effort 回显的互动状态：最近互动过的视频列表。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub items: Vec<InteractionItem>,
}
