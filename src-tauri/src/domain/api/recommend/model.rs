//! 推荐流数据模型（一个推荐 tab 的 cell 定位配置）。

/// 一个推荐 tab 的 cell 配置（cell/change 的两个定位 id，每 tab 不同）。
#[derive(Debug, Clone)]
pub struct RecommendTabConfig {
    pub cell_id: String,
    pub bookstore_id: String,
}
