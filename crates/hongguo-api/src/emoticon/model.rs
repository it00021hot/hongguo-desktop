//! 表情域模型（官方表情 / 第三方 GIF / 自定义表情通用形态；camelCase 对外，
//! snake_case 官方协议由 `parse_emoticon` / `image_data_value` 显式映射）。
//! 三类表情共用同一套字段，差别只在 `image_type` 与可选字段是否在场。

use serde::{Deserialize, Serialize};

/// 表情缩略图（官方缩略图恒带 `dynamic_url`，`web_uri` 恒空串）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonThumb {
    pub web_uri: String,
    pub dynamic_url: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
}

/// 表情条目。三类表情与本地图片共用，字段在场性见模块注释。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Emoticon {
    /// 表情 id（服务端分配）；本地图片评论无此字段。
    #[serde(default)]
    pub id: Option<String>,
    /// 静态图 URL；本地图片评论填上传接口返回的完整签名 URL。
    pub web_uri: String,
    /// 动图 URL；本地图片评论无此字段。
    #[serde(default)]
    pub dynamic_url: Option<String>,
    /// 1 = 静态图 / 2 = 动图。
    pub image_type: i64,
    pub width: i64,
    pub height: i64,
    /// 图片格式（jpeg/png/webp…）；官方表情恒空串。
    #[serde(default)]
    pub format: Option<String>,
    /// 表情名；官方表情恒空串。
    #[serde(default)]
    pub image_name: Option<String>,
    /// 缩略图列表。
    #[serde(default)]
    pub thumb_nails: Option<Vec<EmoticonThumb>>,
    /// 展开大图 URL；自定义表情带，第三方表情不带。
    #[serde(default)]
    pub expand_web_url: Option<String>,
}

/// 表情列表一页（`/reading/ugc/comment/emoticon/v` 响应 `data`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonPage {
    pub items: Vec<Emoticon>,
    pub has_more: bool,
    pub next_offset: i64,
}

/// 图片上传结果（`/reading/reader/upload/picture/` 响应）。
/// `web_uri` 是对象 key，`data` 是可直接引用的完整签名 URL——评论 `image_data.web_uri` 填后者。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureUpload {
    /// 对象 key（`tos-cn-i-<bucket>/<hash>`）。
    pub web_uri: String,
    /// 完整签名 URL（评论附图与收藏表情都引用它）。
    pub url: String,
}

/// 评论附图载荷：`image_data` 条目 + `business_param.emoji_scene_type`
///（2 = 第三方表情/GIF，4 = 自定义表情；本地图片评论不带该字段）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentAttachment {
    /// `image_data` 条目。
    pub image: Emoticon,
    /// `business_param.emoji_scene_type`（本地图片评论为 None）。
    pub emoji_scene_type: Option<i64>,
}

/// 收藏 / 删除表情的请求体（扁平 JSON，6 字段恒在场，body gzip，字段按官方字母序声明保持逐字节一致）。
/// `image_id` 收藏已有表情填源表情 id、上传本地图片为空串；`image_url` 按 `dynamic_url ?: web_url ?: web_uri`。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EmoticonCollectRequest {
    #[serde(rename = "collect_type")]
    pub collect_type: i64,
    #[serde(rename = "image_height")]
    pub image_height: i64,
    #[serde(rename = "image_id")]
    pub image_id: String,
    #[serde(rename = "image_type")]
    pub image_type: i64,
    #[serde(rename = "image_url")]
    pub image_url: String,
    #[serde(rename = "image_width")]
    pub image_width: i64,
}
