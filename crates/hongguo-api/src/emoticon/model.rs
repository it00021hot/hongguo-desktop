//! 表情域模型（官方表情 / 第三方 GIF / 自定义表情通用形态）。
//!
//! 三类表情共用同一套字段，
//! 差别只在 `image_type` 与可选字段是否在场——
//! - 第三方表情（GIF tab，`search_type=2`）：`image_type=2`，带 `id` /
//!   `dynamic_url` / `thumb_nails`，`format` 与 `image_name` 恒空串；
//! - 自定义表情（♡ tab，`search_type=4&keyword=profile`）：`image_type=1`，
//!   带 `id` / `dynamic_url` / `thumb_nails` / `expand_web_url`；
//! - 本地图片评论（`/reading/reader/upload/picture/` 上传后引用）：只有
//!   `web_uri` / 宽高 / `format` / `image_type=1`，其余字段**不在场**。
//!
//! 本文件是**对外（IPC）**形态：camelCase 键，与 crate 内其他输出模型一致；
//! 官方协议侧的 snake_case 键由 [`super::image_data_value`] /
//! [`super::parse_emoticon`] 显式映射（官方请求体字段在场性靠「键在不在」
//! 表达，不能靠空串糊弄）。

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
///
/// 服务端回两个东西：`web_uri` 是对象 key（`tos-cn-i-…`），`data` 是可直接
/// 引用的完整签名 URL——评论 `image_data.web_uri` 填的是后者。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureUpload {
    /// 对象 key（`tos-cn-i-<bucket>/<hash>`）。
    pub web_uri: String,
    /// 完整签名 URL（评论附图与收藏表情都引用它）。
    pub url: String,
}

/// 评论附图载荷：`image_data` 条目 + `business_param.emoji_scene_type`。
///
/// 场景值：2 = 第三方表情/GIF，4 = 自定义表情；
/// 本地图片评论**不带**该字段（`emoji_scene_type` 缺席）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentAttachment {
    /// `image_data` 条目。
    pub image: Emoticon,
    /// `business_param.emoji_scene_type`（本地图片评论为 None）。
    pub emoji_scene_type: Option<i64>,
}

/// 收藏 / 删除表情的请求体（`/reading/ugc/comment/emoticon/collect/v`）。
///
/// 官方形态：扁平 JSON，6 个字段恒在场，body gzip。
///
/// - `collect_type`：1 = 添加（收藏为我的表情）、2 = 删除；
/// - `image_id`：**源表情 id**——收藏官方/第三方表情时非空（收藏他人发的
///   表情即填它的 id），上传本地图片时为空串（图片还没有服务端 id）；
/// - `image_url`：照官方 `ImageData` 规则取 `dynamic_url ?: web_url ?: web_uri`
///   （官方 `oi2.s.o`，`expand_web_url` 不参与）。
///
/// 字段顺序按官方的字母序声明（本仓 `serde_json` 的 Map 同样是字母序，
/// 保持逐字节形态一致）。
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
