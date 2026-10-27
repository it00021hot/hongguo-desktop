//! 推荐流数据模型（推荐 tab 的 cell 定位配置 + 顶栏 tab 列表条目）。

use serde::{Deserialize, Serialize};

/// 一个推荐 tab 的 cell 配置（cell/change 的两个定位 id，每 tab 不同）。
#[derive(Debug, Clone)]
pub struct RecommendTabConfig {
    pub cell_id: String,
    pub bookstore_id: String,
}

/// 顶栏 tab 列表条目（`bookmall/tab` 响应 `tab_item[]` 下发：
/// 顺序即官方顶栏顺序「关注/漫剧/真人剧/推荐」；非活跃 tab
/// 的 bookstore_id 为 null，活跃 tab 才带。关注 45 游客态无数据面，显隐
/// 由壳按登录态过滤，这里原样透传）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedTabMeta {
    pub tab_type: String,
    /// 服务端下发的展示标题（中文原文，壳不做翻译管线）。
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub bookstore_id: Option<String>,
}
