//! 详情域数据模型（相关作品 / 分集列表 / 头部剧集元信息）。

use crate::domain::model::Episode;

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
}

/// 详情页头部的剧集元信息（对齐 hgplayer 头部数据面）。
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
    /// 红果热度值（hot_score，3786万；2026-10-08 抓包实锤与 hgplayer
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
}
