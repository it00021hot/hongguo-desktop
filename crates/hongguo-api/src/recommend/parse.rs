//! 推荐流响应解析。

use serde_json::Value;

use super::cache::flex_str;
use super::session::{push_impressions, rec_impression_of};
use hongguo_common::{AppError, AppResult};

pub(super) fn is_follow_video_cell(cell: &Value) -> bool {
    // APK ShowType::HgFollowContentCell.
    cell.get("show_type").and_then(Value::as_i64) == Some(648)
}

pub(super) fn parse_feed_cell_items(
    cells: Option<&Value>,
    following: bool,
) -> Vec<crate::discover::FeedItem> {
    let mut items = Vec::new();
    for cell in cells.and_then(Value::as_array).into_iter().flatten() {
        if following {
            if let Some(item) = recommend_feed_entry(cell).filter(|item| !item.vid.is_empty()) {
                items.push(item);
            } else if is_follow_video_cell(cell) {
                items.extend(parse_feed_cell_items(cell.get("cell_data"), true));
            }
            continue;
        }
        items.extend(
            cell.get("video_data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(recommend_feed_entry),
        );
    }
    items
}

pub(super) fn parse_recommend_feed(
    data: Option<&Value>,
    following: bool,
) -> AppResult<crate::discover::FeedPage> {
    use crate::discover::FeedPage;
    let data = data.ok_or_else(|| AppError::Media("推荐流响应缺少 data".into()))?;
    let items = parse_feed_cell_items(data.pointer("/cell_view/cell_data"), following);
    Ok(FeedPage {
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        session_id: flex_str(data.get("session_id")).unwrap_or_default(),
        items,
    })
}

/// 一条推荐流条目（cell/change 与 bookmall/tab 嵌套 cell 共用同一形态）。
pub(crate) fn recommend_feed_entry(raw: &Value) -> Option<crate::discover::FeedItem> {
    let det = raw.get("video_detail");
    let video = raw.pointer("/video_detail/video_list/0");
    // 关注作品的 ID 在 video_list，related_album_id 则是底栏关联短剧，不是作品本身。
    let nested_id =
        flex_str(video.and_then(|v| v.get("series_id"))).filter(|s| !s.is_empty() && s != "0");
    let regular_id = [
        raw.get("series_id"),
        raw.pointer("/video_detail/series_id_str"),
        video.and_then(|v| v.get("series_id")),
    ]
    .into_iter()
    .find_map(|id| flex_str(id).filter(|s| !s.is_empty() && s != "0"))?;
    let highlight = det.and_then(|data| {
        crate::highlight::parse(data, Some(raw), nested_id.as_deref().unwrap_or(&regular_id))
    });
    let series_id = if highlight.is_some() {
        nested_id.unwrap_or(regular_id)
    } else {
        regular_id
    };
    let vid_candidates = if highlight.is_some() {
        [video.and_then(|v| v.get("vid")), raw.get("vid")]
    } else {
        [raw.get("vid"), video.and_then(|v| v.get("vid"))]
    };
    // 卡片自带的曝光上下文进最近曝光环（翻页 rec_filter_info_list 的数据源）
    if let Some(impression) = rec_impression_of(raw) {
        push_impressions(std::iter::once(impression));
    }
    let season_tag = det
        .and_then(|d| d.get("secondary_infos"))
        .and_then(Value::as_array)
        .and_then(|arr| {
            // 季徽：data_type 缺省或 0 的第一条（「第1季」，highlight）
            arr.iter().find_map(|it| {
                let dt = it.get("data_type").and_then(Value::as_i64);
                (dt.is_none() || dt == Some(0))
                    .then(|| it.get("content").and_then(Value::as_str))
                    .flatten()
                    .map(str::to_string)
            })
        })
        .unwrap_or_default();
    Some(crate::discover::FeedItem {
        series_id,
        highlight,
        title: det
            .and_then(|d| d.get("series_title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover: det
            .and_then(|d| d.get("series_cover"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                det.and_then(|d| d.get("series_cover_uri"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| video.and_then(|v| v.get("cover")).and_then(Value::as_str))
            .unwrap_or_default()
            .to_string(),
        horiz_cover: String::new(),
        // 推荐位 vid 两个位置都见过：顶层 vid 与 video_detail.video_list[0].vid，
        // 两级回落；批量取流预取的 vid 源就是它
        vid: vid_candidates
            .into_iter()
            .find_map(|id| flex_str(id).filter(|s| !s.is_empty() && s != "0"))
            .unwrap_or_default(),
        episode_cnt: det
            .and_then(|d| d.get("episode_cnt"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as u32,
        play_cnt: det
            .and_then(|d| d.get("series_play_cnt"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        comment_count: 0,
        // 评分（secondary_infos data_type=4，「9.6分」形态；无则 0）
        score: det
            .and_then(|d| d.get("secondary_infos"))
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|it| {
                    (it.get("data_type").and_then(Value::as_i64) == Some(4))
                        .then(|| it.get("content").and_then(Value::as_str))
                        .flatten()
                        .and_then(|s| s.trim_end_matches('分').parse::<f64>().ok())
                })
            })
            .unwrap_or(0.0),
        tags: crate::discover::parse_tags(det.and_then(|d| d.get("category_schema"))),
        season_tag,
        // 热度行：条目级 rec_tags 里 data_type=1 的 content 多段拼接
        //（「共217万人在追」「逆袭榜 No.3」等）
        heat_text: raw
            .get("rec_tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter(|t| t.get("data_type").and_then(Value::as_i64) == Some(1))
                    .filter_map(|t| t.get("content").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default(),
        // 运营角标 pill（「新剧」等；官方随 tag_info.bg_color 下发颜色，前端用主题色即可）
        badge: det
            .and_then(|d| d.pointer("/tag_info/text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        content_type: det
            .and_then(|d| d.get("content_type"))
            .and_then(Value::as_i64)
            .filter(|ct| *ct > 0)
            .or_else(|| {
                video
                    .and_then(|v| v.get("content_type"))
                    .and_then(Value::as_i64)
            })
            .unwrap_or(0),
    })
}
