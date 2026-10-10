//! 发现域数据模型（信息流卡片 / 一页信息流 / 找剧筛选面板与筛选条件）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 信息流的一条剧集卡片。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub series_id: String,
    pub title: String,
    /// 竖版封面（卡片网格用）
    #[serde(default)]
    pub cover: String,
    /// 横版封面（详情头部可选用）
    #[serde(default)]
    pub horiz_cover: String,
    /// 当前推荐位的 vid（直接起播用）
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 播放量（热榜排序键）
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub comment_count: i64,
    #[serde(default)]
    pub score: f64,
    /// 题材标签（来自 category_schema 字符串的二次解析）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 季角标（sub_title_list data_type=0，「第1季」形态；hgplayer titleTag 同源）
    #[serde(default)]
    pub season_tag: String,
    /// 热度文本（sub_title_list data_type=27，「1705万」形态，配火焰图标展示）
    #[serde(default)]
    pub heat_text: String,
    /// 官方运营角标（tag_info.text：「新剧/爆剧/红果首发」等；2026-10-07 抓包实证）
    #[serde(default)]
    pub badge: String,
    /// 内容类型：1=真人剧，1004=漫剧（推荐流「按类型刷」的过滤键）
    #[serde(default)]
    pub content_type: i64,
}

/// 一页信息流。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    pub next_offset: i64,
    pub has_more: bool,
    #[serde(default)]
    pub session_id: String,
}

// ---------------------------------------------------------------- 找剧（筛选浏览）

/// 找剧筛选面板的一行（一个维度）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorItem {
    /// 选项 id（select_items 的取值，如 `short_play`/`cate_262`/`days_7`）
    pub id: String,
    /// 展示名（如 `真人剧`/`脑洞`/`7天内上新`）
    pub name: String,
}

/// 找剧筛选面板的一行。`row_type` 即 select_items 的键
/// （genre/category_dim_theme/category_dim_role/category_dim_epoch/
/// sort/gender/online_time/duration）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorRow {
    pub row_type: String,
    /// 服务端行名（`全部体裁`…，行头「全部」态即空选）
    pub row_name: String,
    pub items: Vec<SelectorItem>,
}

/// 找剧的筛选条件（每维至多一个选中值，空串/None = 全部）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowseFilters {
    pub genre: String,
    pub theme: String,
    pub role: String,
    pub epoch: String,
    /// 推荐：`online_time`(最新上架)/`hot_score`(最高热度)/`hot_collect`(最高收藏)
    pub sort: String,
    /// 受众：`1`=男频 `0`=女频
    pub gender: String,
    /// 上新时间：`days_7`/`days_14`/`days_30`/`days_90`
    pub online_time: String,
    /// 长度：`duration_0_60`/`duration_60_120`/`duration_120_plus`
    pub duration: String,
    /// 完结状态（2026-10-11 逆向 hgplayer 1.1.8 SelectorPanel：客户端
    /// 合成筛选项，选中值走请求体顶层 `creation_status` 参数）：
    /// 空=全部 / `creation_status_0`=已完结 / `creation_status_1`=连载中
    pub creation_status: String,
}

impl BrowseFilters {
    /// select_items 请求形态：每维一个单元素数组（空选给空数组）。
    pub(super) fn to_select_items(&self) -> Value {
        let one = |v: &str| {
            if v.is_empty() {
                Value::Array(vec![])
            } else {
                serde_json::json!([v])
            }
        };
        serde_json::json!({
            "category_dim_epoch": one(&self.epoch),
            "category_dim_role": one(&self.role),
            "category_dim_theme": one(&self.theme),
            "duration": one(&self.duration),
            "gender": one(&self.gender),
            "genre": one(&self.genre),
            "online_time": one(&self.online_time),
            "sort": one(&self.sort),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 筛选条件 → select_items：选中值包单元素数组，空选给空数组。
    #[test]
    fn browse_filters_build_select_items() {
        let f = BrowseFilters {
            genre: "comic_series".into(),
            online_time: "days_7".into(),
            ..Default::default()
        };
        let si = f.to_select_items();
        assert_eq!(si["genre"], serde_json::json!(["comic_series"]));
        assert_eq!(si["online_time"], serde_json::json!(["days_7"]));
        assert_eq!(si["sort"], serde_json::json!([]));
        // duration 维度也要在场（面板有「长度」行，2026-10-07 抓包）
        assert!(si.get("duration").is_some());
        assert_eq!(si["duration"], serde_json::json!([]));
    }
}
