//! 详情域的 JSON 解析（分集列表 / 相关作品 / 头部元信息）。

use serde_json::Value;

use super::model::{EpisodeList, RelatedItem, RelatedSeries, SeriesCreator, SeriesMeta};
use hongguo_common::model::Episode;
use hongguo_common::utils::json::{check_code_in, int_field, str_field, str_field_any};
use hongguo_common::{AppError, AppResult};

/// 解析 plan/v 响应为相关作品两块内容。
pub(super) fn parse_related_series(value: &Value) -> AppResult<RelatedSeries> {
    check_code_in(value, "相关作品接口")?;

    let mut related = RelatedSeries::default();
    for cell in value
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let cell_name = cell.get("cell_name").and_then(Value::as_str).unwrap_or("");
        // 猜你喜欢 cell 的条目不在自己的 video_data 里，而在 cell_data[]（二级 cell）
        // 各自的 video_data 里——只读顶层 video_data 会恒为 0 条。
        let raw_items: Vec<&Value> = match cell.get("video_data").and_then(Value::as_array) {
            Some(arr) if !arr.is_empty() => arr.iter().collect(),
            _ => cell
                .get("cell_data")
                .and_then(Value::as_array)
                .map(|subs| {
                    subs.iter()
                        .filter_map(|sub| sub.get("video_data").and_then(Value::as_array))
                        .flatten()
                        .collect()
                })
                .unwrap_or_default(),
        };
        let items = raw_items
            .iter()
            .filter_map(|raw| {
                let series_id = raw.get("series_id").and_then(Value::as_str)?;
                if series_id.is_empty() {
                    return None;
                }
                // 角标：tag_info.text；缺失且未上线时给「即将上线」
                let tag = raw
                    .pointer("/tag_info/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let episode_cnt = int_field(raw, "episode_cnt").max(0) as u32;
                Some(RelatedItem {
                    series_id: series_id.to_string(),
                    title: str_field(raw, "title"),
                    cover: str_field(raw, "cover"),
                    tag: if tag.is_empty() && episode_cnt == 0 {
                        "即将上线".to_string()
                    } else {
                        tag
                    },
                    // score 线上是字符串形态（"8.0"）
                    score: raw
                        .get("score")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or_else(|| raw.get("score").and_then(Value::as_f64).unwrap_or(0.0)),
                    play_cnt: int_field(raw, "play_cnt"),
                    episode_cnt,
                    video_desc: str_field(raw, "video_desc"),
                })
            })
            .collect();
        match cell_name {
            "相关作品" => related.works = items,
            "猜你喜欢" => related.guess = items,
            _ => {}
        }
    }
    Ok(related)
}

/// 从 detail 响应里解析分集。
pub fn parse_episodes(response: &Value) -> AppResult<EpisodeList> {
    check_code_in(response, "详情接口")?;

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| AppError::Media("响应里没有 data".into()))?;

    let (sid, node) = data.iter().next().expect("data 已确认非空");
    episode_list_of(sid, node)
}

/// 批量形态：`data` 的每个 series 各解析一条（官方预取对 6 部剧打一次
/// `multi_video_detail/preload`）。单部残缺跳过，不拖垮整批。
pub fn parse_episodes_batch(response: &Value) -> AppResult<Vec<EpisodeList>> {
    check_code_in(response, "详情接口")?;

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| AppError::Media("响应里没有 data".into()))?;

    let lists = data
        .iter()
        .filter_map(|(sid, node)| episode_list_of(sid, node).ok())
        .collect::<Vec<_>>();
    if lists.is_empty() {
        return Err(AppError::Media("批量分集列表为空".into()));
    }
    Ok(lists)
}

/// 单剧节点 → `EpisodeList`（`parse_episodes` 的主体抽出，批量共用）。
fn episode_list_of(series_id: &str, node: &Value) -> AppResult<EpisodeList> {
    let video_data = node.get("video_data").cloned().unwrap_or(Value::Null);

    let list = video_data
        .get("video_list")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("响应里没有 video_data.video_list".into()))?;

    let mut episodes: Vec<Episode> = list
        .iter()
        .filter_map(|item| {
            let vid = item.get("vid")?.as_str()?.to_string();
            Some(Episode {
                vid_index: item.get("vid_index").and_then(Value::as_u64).unwrap_or(0) as u32,
                vid,
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                file_stem: String::new(),
                comment_count: item
                    .get("comment_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                digg_count: item
                    .get("digged_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                // 时长（秒，整型；选集格「02:35」角标数据源）
                duration: item.get("duration").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect();

    if episodes.is_empty() {
        return Err(AppError::Media("分集列表为空".into()));
    }
    episodes.sort_by_key(|e| e.vid_index);

    let cover = str_field_any(&video_data, &["series_cover", "cover_url"]);
    Ok(EpisodeList {
        series_id: series_id.to_string(),
        title: str_field_any(&video_data, &["series_title"]),
        cover,
        episodes,
        followed_cnt: video_data
            .get("followed_cnt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        content_type: video_data
            .get("content_type")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

/// 解析 video_detail 响应。字段在 `data[series_id]` 下（与 preload 的
/// `video_data` 包一层不同，这个端点是平铺的），两层都兜一下。
pub fn parse_series_meta(response: &Value, series_id: &str) -> AppResult<SeriesMeta> {
    check_code_in(response, "详情元信息接口")?;
    let node = response
        .pointer(&format!("/data/{series_id}"))
        .or_else(|| {
            response
                .get("data")
                .and_then(|d| d.as_object().and_then(|o| o.values().next()))
        })
        .ok_or_else(|| AppError::Media("详情元信息响应里没有 data".into()))?;
    let vd = node.get("video_data").unwrap_or(node);

    // secondary_infos：data_type 0 = 季徽（highlight），3 = 题材标签
    let mut season = String::new();
    let mut tags = Vec::new();
    if let Some(items) = node
        .pointer("/secondary_infos")
        .or_else(|| vd.pointer("/secondary_infos"))
        .and_then(Value::as_array)
    {
        for item in items {
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            match item.get("data_type").and_then(Value::as_i64) {
                Some(0) if season.is_empty() => season = content.to_string(),
                Some(3) => tags.push(content.to_string()),
                _ => {}
            }
        }
    }

    let sid = str_field_any(vd, &["series_id_str", "series_id"]);
    Ok(SeriesMeta {
        series_id: if sid.is_empty() {
            series_id.to_string()
        } else {
            sid
        },
        title: str_field_any(vd, &["series_title"]),
        cover: str_field_any(vd, &["series_cover"]),
        followed_cnt: vd
            .get("followed_cnt")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("followed_cnt")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        play_cnt: vd
            .get("series_play_cnt")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("series_play_cnt")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        hot_score: vd
            .get("hot_score")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("hot_score")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        record_number: vd
            .pointer("/record_info/record_number")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        season,
        tags,
        intro: str_field_any(vd, &["series_intro", "video_desc", "abstract"]),
        // 官方模型对照字段：防御性读取，缺席走默认值
        pay_info: vd.get("pay_info").cloned().unwrap_or(Value::Null),
        // 官方下发为 unix 秒；对外契约统一 UTC 毫秒
        online_time: int_field(vd, "online_time") * 1000,
        update_tag: str_field_any(vd, &["episode_right_text", "update_tag"]),
        // 追更按钮显隐判定：0=连载中 1=已完结；缺席按连载中处理
        series_status: int_field(vd, "series_status"),
        update_frequency: vd
            .pointer("/update_info/show_update_frequency")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        episode_total_cnt: int_field(vd, "episode_total_cnt"),
        latest_consumed_vid: str_field_any(vd, &["latest_consumed_item_id"]),
        creators: parse_creators(vd),
    })
}

/// 解析出品方 / 演职员（main_creator_users）：base_info 出名字头像，
/// user_tag.sub_title_list 首项出角色文案。名字为空的条目跳过。
fn parse_creators(vd: &Value) -> Vec<SeriesCreator> {
    let mut out = Vec::new();
    for raw in vd
        .get("main_creator_users")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = raw
            .pointer("/base_info/user_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(SeriesCreator {
            user_id: raw
                .pointer("/base_info/user_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: name.to_string(),
            avatar: raw
                .pointer("/base_info/user_avatar")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            role: raw
                .pointer("/user_tag/sub_title_list/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    out
}
