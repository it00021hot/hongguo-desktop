//! 搜索域数据模型（联想片段 / 联想条目 / 搜索结果 / 一页搜索）。

use serde::{Deserialize, Serialize};

/// 联想词的一个渲染片段（hgplayer 同款 TextPart：按命中位切开，
/// hl=true 的片段前端上高亮色）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestPart {
    pub text: String,
    pub hl: bool,
}

/// 一条搜索联想（query_result_v2 形态）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestItem {
    /// 联想词（= 剧名；v2 的 name）
    pub word: String,
    /// 命中高亮切片（search_high_light.high_light_position 切 name 所得；
    /// 无高亮信息为空，前端整体按普通文本渲染）
    #[serde(default)]
    pub parts: Vec<SuggestPart>,
    /// 对应剧集 id（= keyword；无 video_data 的纯词联想为空串，
    /// 前端回落为「以该词发起搜索」）
    #[serde(default)]
    pub series_id: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub cover: String,
    /// 摘要行（「第1季·玄幻·4105万热度」，v2 的 sug_abstract）
    #[serde(default, rename = "abstract")]
    pub abstract_text: String,
}

/// 一条搜索结果（形状同榜单条目，省去榜单专属字段）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub sub_title: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    #[serde(default)]
    pub description: String,
}

/// 一页搜索结果。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<SearchResult>,
    pub has_more: bool,
    pub next_offset: i64,
    /// 首页响应发放的会话 id，翻页时原样带回。
    #[serde(default)]
    pub search_id: String,
}
