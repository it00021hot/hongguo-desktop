//! 发现域的 JSON 解析（信息流 / 找剧面板组装）。
//!
//! 跨域共享的字段与错误码助手（check_code/str_field/int_field/num_field/
//! parse_tags）已收敛进 `crate::utils::json`（P3-C9），经本域 mod.rs
//! 再导出，保持 `super::discover::X` 的既有引用路径不变。

use serde_json::Value;

use super::model::{FeedItem, FeedPage, SelectorItem, SelectorRow};
use crate::error::{AppError, AppResult};
use crate::utils::json::{int_field, num_field, parse_tags, str_field};

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
        // sub_title_list：data_type 0=季文本 / 3=分类 / 27=热度（2026-10-07
        // 抓包实证；分类沿用 category_schema 解析，这里只取季与热度）
        let (season_tag, heat_text) = parse_sub_titles(raw.get("sub_title_list"));
        // 官方运营角标：tag_info（同名字段在 plan/v 里是「第N季/同IP」，
        // 在 landpage 信息流里是「新剧/爆剧/红果首发」，enable=false 不显）
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

/// sub_title_list → (季文本, 热度文本)。data_type 语义见 2026-10-07 抓包：
/// 0=「第1季」形态、27=热度数值文本（官方配火焰图标）、3=分类（另有
/// category_schema 承载，这里不取）。两条都算展示增强，缺了给空串。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_feed_page_with_nested_category_schema() {
        let data: Value = serde_json::json!({
            "has_more": true,
            "next_offset": 15,
            "session_id": "s1",
            "video_data": [
                {
                    "series_id": "1001",
                    "title": "剧 A",
                    "cover": "c",
                    "horiz_cover": "h",
                    "vid": "v1",
                    "episode_cnt": 80,
                    "play_cnt": "12345678",   // 字符串形态的大数
                    "comment_count": 42,
                    "score": 8.7,
                    "category_schema": "[{\"category_id\":1,\"name\":\"逆袭\"},{\"category_id\":2,\"name\":\"穿越\"}]"
                },
                { "title": "没有 id 的废条目" }
            ]
        });
        let page = parse_feed(Some(&data)).unwrap();
        assert_eq!(page.items.len(), 1, "缺 series_id 的条目要跳过");
        let item = &page.items[0];
        assert_eq!(item.series_id, "1001");
        assert_eq!(item.play_cnt, 12_345_678, "字符串数字要能读");
        assert_eq!(item.score, 8.7);
        assert_eq!(item.tags, vec!["逆袭", "穿越"]);
        assert!(page.has_more);
        assert_eq!(page.next_offset, 15);
    }

    #[test]
    fn broken_category_schema_degrades_to_empty_tags() {
        let data: Value = serde_json::json!({
            "video_data": [{ "series_id": "1", "title": "t", "category_schema": "not json" }]
        });
        let page = parse_feed(Some(&data)).unwrap();
        assert!(page.items[0].tags.is_empty());
    }

    /// 面板解析：行 type/row_name/选项 id+名（2026-10-07 抓包样本的形状）。
    #[test]
    fn parses_browse_panel_rows() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": {
                "selector_rows": [
                    {
                        "type": "genre",
                        "row_name": "全部体裁",
                        "selection_type": 2,
                        "items": [
                            { "selector_item_id": "short_play", "show_name": "真人剧" },
                            { "selector_item_id": "comic_series", "show_name": "漫剧" },
                            { "selector_item_id": "ai_series", "show_name": "AI剧" }
                        ]
                    },
                    {
                        "type": "sort",
                        "row_name": "全部推荐",
                        "items": [
                            { "selector_item_id": "online_time", "show_name": "最新上架" }
                        ]
                    },
                    { "type": "", "row_name": "坏行", "items": [] }
                ]
            }
        });
        let rows = parse_browse_panel(v.get("data")).unwrap();
        assert_eq!(rows.len(), 2, "空 type 的行要跳过");
        assert_eq!(rows[0].row_type, "genre");
        assert_eq!(rows[0].items.len(), 3);
        assert_eq!(rows[0].items[0].id, "short_play");
        assert_eq!(rows[0].items[0].name, "真人剧");
        assert_eq!(rows[1].row_type, "sort");
    }
}
