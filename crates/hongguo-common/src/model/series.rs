//! 剧集领域模型。
//!
//! 字段一律 camelCase：落库整行存 JSON 列、走 IPC 都用同一形态（前端 `schema.ts` 同步声明 camelCase）。

use serde::{Deserialize, Serialize};

/// Playable creator video, distinct from the drama it may promote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub author: String,
    pub related: Option<RelatedDrama>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedDrama {
    pub series_id: String,
    /// Official bottom-bar copy, which may contain an episode count or genre.
    pub label: String,
    #[serde(default)]
    pub kind: RelatedKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RelatedKind {
    #[default]
    Drama,
    Album,
}

/// 一部短剧。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub episode_count: u32,
    /// 全剧收藏数（detail 响应 video_data.followed_cnt，右栏「☆ N」数据源；
    /// 旧档案缓存缺省 0 = 不显示）
    #[serde(default)]
    pub followed_cnt: i64,
    /// 剧的条目类型（官方 content_type：漫剧 1004 / 真人剧 1 / 书城条目 1001…）：真播批量分组 key 与单集请求 content_type 的持久来源，起播时整剧各集按它注册进取流注册表；0 = 未知（旧档案缓存），取流走单集占位值（服务端对该字段容错）。
    #[serde(default)]
    pub content_type: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Box<Highlight>>,
    /// 完整分集（拉取后写入）
    #[serde(default)]
    pub episodes: Vec<Episode>,
    /// 用户主动从列表移除的标记。移除列表与删除文件是两个独立操作。
    #[serde(default)]
    pub dismissed: bool,
}

/// 一集。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    /// 集号，从 1 开始
    pub vid_index: u32,
    /// 平台侧的 vid
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub title: String,
    /// 落盘后的文件名（不含扩展名）
    pub file_stem: String,
    /// 该集评论数（detail 响应公开计数，右栏「💬 N」数据源；旧档案缓存缺省 0）
    #[serde(default)]
    pub comment_count: i64,
    /// 该集点赞数（detail 响应公开计数，右栏「♥ N」数据源）
    #[serde(default)]
    pub digg_count: i64,
    /// 该集时长（秒；detail 响应 video_list[].duration，详情页选集格展示用，
    /// 旧档案缓存缺省 0）
    #[serde(default)]
    pub duration: i64,
}
