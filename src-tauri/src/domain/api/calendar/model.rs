//! 上新日历 / 预约列表数据模型（剧集条目与分页，两域共用同一端点响应）。

use serde::{Deserialize, Serialize};

/// 上新日历的一条剧集。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarItem {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 简介（video_desc）
    #[serde(default)]
    pub description: String,
    /// 主分类（category，如 "逆袭"）
    #[serde(default)]
    pub category: String,
    /// 热度文案（rec_tags[].content，如 "374万热度"）
    #[serde(default)]
    pub rec_tags: Vec<String>,
    /// 排期上线时间（unix 秒；0 = 未定档）
    #[serde(default)]
    pub publish_time: i64,
    /// 是否已上线
    #[serde(default)]
    pub is_online: bool,
    /// 当前账号是否已预约（预约列表扁平形态下发；日历形态恒 false）
    #[serde(default)]
    pub has_subscribed: bool,
}

/// 上新日历页。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarPage {
    pub items: Vec<CalendarItem>,
    /// 可选日期（"20261003" 形式，接口给前后各一周）
    pub dates: Vec<String>,
    /// 默认选中日期
    #[serde(default)]
    pub default_date: String,
    /// 还有下一页（一周的条目按排期升序分布在多页里）
    #[serde(default)]
    pub has_more: bool,
    /// 下一页 offset（0 = 没有更多）
    #[serde(default)]
    pub next_offset: i64,
    /// 预约列表（tab_type=13）的两个 tab 计数；日历形态恒 0
    #[serde(default)]
    pub online_total: i64,
    #[serde(default)]
    pub offline_total: i64,
    /// 服务端下发的浏览会话标识：翻页续传用（响应下发、翻页时带回；
    /// 首页请求绝不能自带，否则命中服务端会话缓存——见 fetch_reservations）
    #[serde(default)]
    pub session_id: String,
}
