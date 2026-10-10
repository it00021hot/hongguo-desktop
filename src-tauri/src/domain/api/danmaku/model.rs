//! 弹幕域数据模型（一条弹幕 / 评论区条目 / 评论页 / 剧评页+评分摘要）。

use serde::{Deserialize, Serialize};

/// 一条弹幕。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Danmaku {
    pub comment_id: String,
    pub text: String,
    /// 出现时间（毫秒，视频内时间轴）
    #[serde(default)]
    pub offset_ms: u64,
    #[serde(default)]
    pub digg_count: i64,
}

/// 一条评论区评论（用户资料 / 计数 / 我的点赞态，见 2026-10-05 抓包样本）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentItem {
    pub comment_id: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub avatar: String,
    pub text: String,
    /// unix 秒
    #[serde(default)]
    pub create_time: i64,
    #[serde(default)]
    pub digg_count: i64,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub user_digg: bool,
    /// 剧评评分（expand.score，"7" 十分制字符串；单集评论恒空串）
    #[serde(default)]
    pub score: String,
    /// 评分后缀文案（expand.score_suffix_text，"观看1小时后点评"；单集评论恒空串）
    #[serde(default)]
    pub score_suffix_text: String,
}

/// 评论区拉取结果：评论列表 + 评论总数（互动栏计数数据源）+ 翻页游标。
///
/// 2026-10-06 改版：**按页拉取**（一次一窗 20 条）——此前把整集全部
/// 拉完才返回，热门集几百页要转圈半分钟（3716 条实测 ~30s），hgplayer
/// 是第一页秒开。total 从第一页的 `common_list_info.total` 取。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentPage {
    pub items: Vec<CommentItem>,
    /// 该集评论总数（`common_list_info.total`，`need_count: true` 回传）
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: String,
}

/// 剧级评论页 + 剧评分摘要。
///
/// 评分/评分人数在本接口响应的 `extra` 里，字段是 **credibility_score /
/// credibility_score_count**（2026-10-08 抓包实锤：《修仙：众人看我舔疯癫》
/// `credibility_score=8.3`、`credibility_score_count=1247`，与 hgplayer
/// 头部「8.3分 1247人评分」逐一吻合；同响应的 `book_info.score` 恒为空串，
/// 此前读它导致头部评分永远不显示。score 是 JSON 数字形态，字符串也兜）。
/// `book_info.tags`（逗号分隔串）是书维度的题材标签，与详情头部的
/// secondary_infos 不是一套。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesReviewPage {
    #[serde(flatten)]
    pub page: CommentPage,
    /// 剧评分（"8.3"；空串 = 暂无评分）
    #[serde(default)]
    pub score: String,
    /// 评分人数
    #[serde(default)]
    pub score_cnt: i64,
    /// 题材标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 剧均评分（extra.book_info.score，"8.6"；空串 = 暂无评分）
    #[serde(default)]
    pub avg_score: String,
}
