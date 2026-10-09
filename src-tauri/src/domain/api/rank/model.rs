//! 排行榜数据模型（榜单条目 / 筛选面板 / 顶部内容 tab / 一页榜单）。

use serde::{Deserialize, Serialize};

/// 榜单条目：比信息流卡片多排名与榜单文案。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankItem {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    /// 榜单名次（recommend_info.rank，从 1 起）
    pub rank: u32,
    /// "玄幻·全200集" 形态的副标题
    #[serde(default)]
    pub sub_title: String,
    /// 评分（"8.0"，容错字符串）
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 榜单热点文案（rec_text_item.RecommendText，如 "13707万最高热度"）
    #[serde(default)]
    pub rec_text: String,
    /// 次级信息（secondary_info_list[].content，如 "258.7万收藏"）
    #[serde(default)]
    pub secondary_infos: Vec<String>,
    /// 简介（video_desc）
    #[serde(default)]
    pub description: String,
    /// 题材标签（category_schema 二次解析）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 未上线（分集数为 0）。hgplayer 同款状态分支：未上线行显示预约
    /// 按钮、已上线行显示播放按钮——按剧状态分，不看榜单 id
    #[serde(default)]
    pub upcoming: bool,
    /// 当前账号已预约（榜单条目自带 online_subscribed；匿名恒 false）
    #[serde(default)]
    pub reserved: bool,
    /// 季徽（sub_title_list 里「第N季」形态条目；标题旁小徽，无则空）
    #[serde(default)]
    pub season: String,
}

/// 排行榜筛选面板的一个选项（id 为空 = 「总榜」，清除筛选）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPanelItem {
    pub id: String,
    pub name: String,
}

/// 筛选面板的一行（row_name：综合/时代背景/主题情节/角色设定…）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPanelRow {
    pub name: String,
    pub items: Vec<RankPanelItem>,
}

/// 内容 tab 下的一个子榜（自带筛选面板 schema）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankSubList {
    pub id: String,
    pub name: String,
    /// 子榜描述行（sub_title，如 "10月4日已更新·基于红果观看/互动以及
    /// 个人兴趣排序"；官方显示在子榜名旁）
    #[serde(default)]
    pub description: String,
    pub panel: Vec<RankPanelRow>,
}

/// 顶部内容 tab（全部/真人剧/漫剧/AI剧/演员/系列剧，2026-10-05 抓包 +
/// `cell_selector` schema 锁定）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankTab {
    pub id: String,
    pub name: String,
    pub subs: Vec<RankSubList>,
}

/// 一页榜单。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPage {
    pub items: Vec<RankItem>,
    /// 内容 tab / 子榜 / 筛选面板的完整选项表（响应 cell_selector 原样
    /// 展开，每次请求都随行下发；前端首次拿到后即可渲染整套筛选 UI）
    #[serde(default)]
    pub tabs: Vec<RankTab>,
    /// 分页游标（每页固定 20 条；has_more=false 或 next_offset=0 到底）
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_offset: i64,
    /// 浏览会话标识：首页响应下发、翻页原样回传（服务端按它维持榜单
    /// 上下文，2026-10-09 抓包实锤）
    #[serde(default)]
    pub session_id: String,
}
