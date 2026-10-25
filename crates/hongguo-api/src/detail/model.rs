//! 详情域数据模型（相关作品 / 分集列表 / 头部剧集元信息）。

use hongguo_common::model::Episode;

/// 相关作品 / 猜你喜欢里的一条。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedItem {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    /// 角标文案（`第1季`/`同IP`…，服务端下发，无则空）
    pub tag: String,
    /// 评分（0 = 无分）
    pub score: f64,
    pub play_cnt: i64,
    /// 0 = 未上线（「即将上线」态）
    pub episode_cnt: u32,
    pub video_desc: String,
}

/// 详情页相关推荐 tab 的两块内容。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedSeries {
    /// 相关作品·系列（同系列各季 + 同 IP）
    pub works: Vec<RelatedItem>,
    /// 猜你喜欢（plan 响应可能给空 cell，空时前端回落既有推荐源）
    pub guess: Vec<RelatedItem>,
}

/// 一部剧的元信息 + 分集。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeList {
    /// 平台侧的剧集 id
    pub series_id: String,
    /// 剧名
    pub title: String,
    /// 封面
    pub cover: String,
    /// 分集，按 `vid_index` 升序
    pub episodes: Vec<Episode>,
    /// 全剧收藏数（video_data.followed_cnt，右栏「☆ N」数据源）
    pub followed_cnt: i64,
    /// 剧的条目类型（video_data.content_type：漫剧 1004 / 真人剧 1 / 书城
    /// 条目 1001…）。真播批量分组 key 与单集请求 content_type 的持久来源；
    /// 0 = 响应未带（取流走单集占位值）。
    pub content_type: i64,
}

/// 详情页头部的剧集元信息。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesMeta {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    /// 追剧数（followed_cnt，44.7万人追剧）
    pub followed_cnt: i64,
    /// 全剧播放量（series_play_cnt，150.8万次播放）
    pub play_cnt: i64,
    /// 红果热度值（hot_score，3786万；2026-10-08 抓包实锤与客户端
    /// 头部「🔥红果热度值3786万」同源同值）
    pub hot_score: i64,
    /// 备案号（record_info.record_number；响应里有 show 开关，前端恒显即可）
    pub record_number: String,
    /// 季徽（secondary_infos data_type=0 的 content，如「第1季」）
    pub season: String,
    /// 题材标签（secondary_infos data_type=3 的 content：玄幻/逆袭/修真…）
    pub tags: Vec<String>,
    /// 剧情简介（series_intro，缺失退 video_desc/abstract；详情页与
    /// 播放器简介面板共用，取代旧官网 extras 链路）
    pub intro: String,
    /// 付费信息（官方模型 VideoDetailVideoData.payInfo 原样透传；
    /// Null=免费。官方模型对照新增，抓包样本未及、防御性读取）
    pub pay_info: serde_json::Value,
    /// 上线时间戳（online_time；UTC 毫秒，0=未知）
    pub online_time: i64,
    /// 官方统计行原文（episode_right_text：完结「全139集」/ 连载「更新至83集」，
    /// 2026-10-15 官方实包；旧字段名 update_tag 在真实响应里不存在，留作兜底键）
    pub update_tag: String,
    /// 追更按钮显隐判定（series_status：0=连载中 1=已完结；与追更列表
    /// relation/get_user_action 同名同语义。官方详情页只在连载剧出「追更」）
    pub series_status: i64,
    /// 更新频率（update_info.show_update_frequency，「每周六更新5集」；
    /// update_info 对象只在连载剧下发，完结剧为空）
    pub update_frequency: String,
    /// 全剧总集数（episode_total_cnt；0=未知）
    pub episode_total_cnt: i64,
    /// 服务端续播锚点 vid（latest_consumed_item_id；空=无）
    pub latest_consumed_vid: String,
    /// 出品方 / 演职员（main_creator_users，2026-10-15 抓 player v1.2.1
    /// 详情页实包；官方点击走 Lynx 模板页无 JSON 端点，展示 + 跳搜索）
    #[serde(default)]
    pub creators: Vec<SeriesCreator>,
}

/// 出品方 / 演职员条目（main_creator_users[] 摘要）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCreator {
    /// 官方账号 uid
    pub user_id: String,
    /// 名称（出品方名 / 演员名）
    pub name: String,
    /// 头像（官方 CDN，直连可渲染）
    pub avatar: String,
    /// 角色文案（user_tag.sub_title_list 首项：出品方/主演/配角）
    pub role: String,
}
