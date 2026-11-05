//! 排行榜响应解析：selector 展开与 video_data 平铺。

use serde_json::Value;

use super::model::{RankItem, RankPanelItem, RankPanelRow, RankSubList, RankTab};
use crate::discover::{int_field, num_field, str_field};
use hongguo_common::{AppError, AppResult};

/// `rank_version` 的容错提取（响应是 JSON 数字，请求回传字符串形态）。
pub(super) fn rank_version_field(data: Option<&Value>) -> Option<String> {
    let v = data?.get("rank_version")?;
    v.as_str()
        .map(str::to_string)
        .or_else(|| v.as_i64().map(|n| n.to_string()))
}

/// selector 面板行展开（panel_selector / background_selector 同构共用）。
pub(super) fn parse_selector_rows(rows: &[Value]) -> Vec<RankPanelRow> {
    rows.iter()
        .filter_map(|row| {
            let items: Vec<RankPanelItem> = row
                .get("items")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|it| RankPanelItem {
                            id: str_field(it, "selector_item_id"),
                            name: str_field(it, "show_name"),
                        })
                        .collect()
                })
                .unwrap_or_default();
            if items.is_empty() {
                return None;
            }
            Some(RankPanelRow {
                name: str_field(row, "row_name"),
                items,
            })
        })
        .collect()
}

/// 展开 `data.cell_view.cell_selector` 的 tab → 子榜 → 面板选项表。
///
/// 结构形态由客户端自报版本号决定：较新版本是两级全量结构，过旧版本
/// 退化成扁平一级结构（一级形态的兼容归一在前端 normalizeTabs）。
pub(crate) fn parse_cell_selector(data: Option<&Value>) -> Vec<RankTab> {
    let Some(tabs) = data
        .and_then(|d| d.get("cell_view"))
        .and_then(|cv| cv.get("cell_selector"))
        .and_then(|cs| cs.get("outer_row"))
        .and_then(|r| r.get("items"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    tabs.iter()
        .filter_map(|tab| {
            let id = str_field(tab, "selector_item_id");
            if id.is_empty() {
                return None;
            }
            let subs = tab
                .get("sub_cell_selector")
                .and_then(|s| s.get("outer_row"))
                .and_then(|r| r.get("items"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|sub| {
                            let sid = str_field(sub, "selector_item_id");
                            // 空 id 但带 panel_selector 的是异常二级子榜；
                            // 空 id 叶子是「全部」哨兵，必须保留
                            if sid.is_empty() && sub.get("panel_selector").is_some() {
                                return None;
                            }
                            let panel = sub
                                .get("panel_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| parse_selector_rows(rows))
                                .unwrap_or_default();
                            let background = sub
                                .get("background_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| parse_selector_rows(rows))
                                .unwrap_or_default();
                            Some(RankSubList {
                                id: sid,
                                name: str_field(sub, "show_name"),
                                description: str_field(sub, "sub_title"),
                                panel,
                                background,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(RankTab {
                name: str_field(tab, "show_name"),
                id,
                subs,
            })
        })
        .collect()
}

/// 遍历 `data` 下所有 `video_data` 数组（cell_data 嵌套结构逐块平铺）。
pub(super) fn collect_video_data(node: &Value, out: &mut Vec<Value>) {
    if let Some(arr) = node.as_array() {
        for item in arr {
            collect_video_data(item, out);
        }
        return;
    }
    let Some(obj) = node.as_object() else { return };
    if let Some(vd) = obj.get("video_data").and_then(Value::as_array) {
        out.extend(vd.iter().cloned());
    }
    for (_k, v) in obj {
        collect_video_data(v, out);
    }
}

/// cell_view 结构 → 平铺榜单条目。
pub(crate) fn parse_rank_items(data: Option<&Value>) -> AppResult<Vec<RankItem>> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut raws = Vec::new();
    collect_video_data(data, &mut raws);

    let mut items = Vec::new();
    for raw in &raws {
        let Some(series_id) = raw
            .get("series_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let rank = raw
            .get("recommend_info")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|info| int_field(&info, "rank").try_into().ok())
            .unwrap_or(0);
        let rec_text = raw
            .get("rec_text_item")
            .and_then(|t| t.get("RecommendText"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let secondary_infos = raw
            .get("secondary_info_list")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("content").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        items.push(RankItem {
            series_id: series_id.to_string(),
            title: str_field(raw, "title"),
            cover: str_field(raw, "cover"),
            vid: str_field(raw, "vid"),
            rank,
            sub_title: str_field(raw, "sub_title"),
            score: num_field(raw, "score"),
            play_cnt: int_field(raw, "play_cnt"),
            // 预约榜条目分集数为 0（已上线剧恒 >0）
            upcoming: int_field(raw, "episode_cnt") == 0,
            reserved: raw
                .get("online_subscribed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            season: raw
                .get("sub_title_list")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .filter_map(|x| x.get("content").and_then(Value::as_str))
                        .find(|c| c.starts_with('第') && c.ends_with("季"))
                        .map(str::to_string)
                })
                .unwrap_or_default(),
            episode_cnt: int_field(raw, "episode_cnt").max(0) as u32,
            rec_text,
            secondary_infos,
            description: str_field(raw, "video_desc"),
            tags: crate::discover::parse_tags(raw.get("category_schema")),
        });
    }
    // 交付序就是官方榜单序，绝不能按 recommend_info.rank 排序——
    // 它是页内位次（每页都是 1~10），rank 只当「有无名次」的标记
    Ok(items)
}
