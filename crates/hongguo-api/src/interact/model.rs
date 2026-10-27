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
    /// 追剧数（右栏 ☆ 下的 21.1万）
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

/// 追更列表里的一部剧（get_user_action action_channel=9 的
/// `mix_data[].video`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpdateItem {
    /// series_id（`video_id`，字符串形态）
    pub series_id: String,
    #[serde(default)]
    pub title: String,
    /// 封面（服务端下发 .heic，展示走封面转码协议）
    #[serde(default)]
    pub cover: String,
    /// 服务端记录的总集数（0 = 未下发）
    #[serde(default)]
    pub episode_cnt: i64,
    /// 服务端记录的已看到第几集（0 = 没看过）
    #[serde(default)]
    pub latest_watched_vid_index: i64,
    /// true = 已完结（series_status==1）
    #[serde(default)]
    pub finished: bool,
    /// 是否已看最新一集
    #[serde(default)]
    pub has_watched_latest_episode: bool,
    /// 追更时间（unix 秒；0 = 未下发）
    #[serde(default)]
    pub follow_update_at: i64,
}

/// 追更列表一页。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpdatePage {
    pub items: Vec<FollowUpdateItem>,
    /// 下页 cursor（服务端下发的 JSON 串**原样**回传；空 = 没有更多。
    /// session_id 协议同预约列表：首页不自造，翻页续传响应下发的值）
    pub cursor: String,
    pub has_more: bool,
}

/// 评论 @ 提及候选里的一条（relation/list 的 ugc_user_list 摘要；
/// 列表含官方 AI bot「小果」）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionUser {
    pub user_id: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub avatar: String,
    /// true = 官方 AI bot（@ 它会触发 AI 回复）
    #[serde(default)]
    pub is_ai_bot: bool,
}
