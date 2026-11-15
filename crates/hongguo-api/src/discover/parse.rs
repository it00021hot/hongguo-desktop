//! 发现域的 JSON 解析（信息流 / 找剧面板组装）。

use serde_json::Value;

use super::model::{FeedItem, FeedPage, SelectorItem, SelectorRow};
use hongguo_common::utils::json::{int_field, num_field, parse_tags, str_field};
use hongguo_common::{AppError, AppResult};

/// 解析面板 data 节点为 selector 行列表。
pub(super) fn parse_browse_panel(data: Option<&Value>) -> AppResult<Vec<SelectorRow>> {
    let data = data.ok_or_else(|| AppError::Media("面板响应缺少 data".into()))?;
    let mut rows = Vec::new();
    for raw in data
        .get("selector_rows")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let row_type = str_field(raw, "type");
        if row_type.is_empty() {
            continue;
        }
        let items = raw
            .get("items")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| {
                        let id = it.get("selector_item_id").and_then(Value::as_str)?;
                        Some(SelectorItem {
                            id: id.to_string(),
                            name: it
                                .get("show_name")
                                .and_then(Value::as_str)
                                .unwrap_or(id)
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.push(SelectorRow {
            row_type,
            row_name: str_field(raw, "row_name"),
            items,
        });
    }
    Ok(rows)
}

/// 解析 data 节点为 FeedPage。
pub(super) fn parse_feed(data: Option<&Value>) -> AppResult<FeedPage> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut items = Vec::new();
    for raw in data
        .get("video_data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // 缺 series_id 的条目没有落地价值（点不开、下不了）
        let Some(series_id) = raw.get("series_id").and_then(Value::as_str) else {
            continue;
        };
        if series_id.is_empty() {
            continue;
        }
        // sub_title_list：data_type 0=季文本 / 3=分类（走 category_schema）/ 27=热度
        let (season_tag, heat_text) = parse_sub_titles(raw.get("sub_title_list"));
        // 官方运营角标：tag_info.text（landpage 里是「新剧/爆剧」等，enable=false 不显）
        let badge = raw
            .pointer("/tag_info/text")
            .and_then(Value::as_str)
            .filter(|_| raw.pointer("/tag_info/enable").and_then(Value::as_bool) != Some(false))
            .unwrap_or("")
            .to_string();
        items.push(FeedItem {
            series_id: series_id.to_string(),
            title: str_field(raw, "title"),
            cover: str_field(raw, "cover"),
            horiz_cover: str_field(raw, "horiz_cover"),
            vid: str_field(raw, "vid"),
            episode_cnt: int_field(raw, "episode_cnt").max(0) as u32,
            play_cnt: int_field(raw, "play_cnt"),
            comment_count: int_field(raw, "comment_count"),
            score: num_field(raw, "score"),
            tags: parse_tags(raw.get("category_schema")),
            season_tag,
            heat_text,
            badge,
            content_type: int_field(raw, "content_type"),
        });
    }
    Ok(FeedPage {
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: int_field(data, "next_offset"),
        session_id: str_field(data, "session_id"),
        items,
    })
}

/// sub_title_list → (季文本, 热度文本)。data_type 语义：0=季文本、27=热度数值文本、
/// 3=分类（category_schema 承载，这里不取）。两条都是展示增强，缺了给空串。
fn parse_sub_titles(list: Option<&Value>) -> (String, String) {
    let mut season = String::new();
    let mut heat = String::new();
    for it in list
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(content) = it.get("content").and_then(Value::as_str) else {
            continue;
        };
        match it.get("data_type").and_then(Value::as_i64) {
            Some(0) if season.is_empty() => season = content.to_string(),
            Some(27) if heat.is_empty() => heat = content.to_string(),
            _ => {}
        }
    }
    (season, heat)
}
