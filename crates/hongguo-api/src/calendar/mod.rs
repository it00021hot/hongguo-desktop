//! 上新日历接口（subscribe/list 的日历形态：`tab_type=5` +
//! `need_calendar_schema=true`；与预约列表共用同一端点，响应解析也在本域，
//! 详见 docs/hongguo-api-endpoints.md）。

mod filtered;
mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use hongguo_common::utils::json::{check_code, int_field, num_field, str_field};
use hongguo_common::utils::time::beijing_date;
use hongguo_common::{AppError, AppResult};

pub use crate::reservation::selectors::{SelectorId, SelectorRow, SelectorSelection};
pub use filtered::{
    CalendarCursor, CalendarFilteredPage, CalendarPanel, fetch_calendar_panel,
    fetch_filtered_calendar,
};
pub use model::{CalendarItem, CalendarPage};

/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// 上新日历（subscribe/list 日历形态）：首页必须带 `need_personal_recommend=1`；
/// 翻页三键齐传 offset/subscribe_offset/session_id（只传 offset 永远回同一页）。
/// 响应是多天混排，按展示日过滤后才是一天的条目。
pub async fn fetch_new_calendar(
    date: Option<&str>,
    cursor: Option<(i64, &str)>,
    env: &ApiEnv,
) -> AppResult<CalendarPage> {
    let base: Vec<(String, String)> = [
        ("active_panel", "6"),
        ("gender_type", "2"),
        ("need_calendar_schema", "true"),
        ("need_personal_recommend", "1"),
        ("tab_style", "2"),
        ("tab_type", "5"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    // 首页（默认日）不带 target_date（官方首页无此键）；切日期才带
    let active_day_input = date.filter(|d| !d.is_empty()).map(str::to_string);
    let mut q = base.clone();
    if let Some(day) = &active_day_input {
        q.push(("target_date".to_string(), day.clone()));
    }
    // 翻页三键齐传（只传 offset 服务端无视，永远回同一页）；
    // 首页请求绝不能自带 session_id（命中服务端会话缓存）
    if let Some((offset, session_id)) = &cursor {
        q.push(("offset".to_string(), offset.to_string()));
        q.push(("subscribe_offset".to_string(), offset.to_string()));
        if !session_id.is_empty() {
            q.push(("session_id".to_string(), (*session_id).to_string()));
        }
    }
    let page = fetch_calendar_page(&q, env).await?;

    // 展示日：显式选择的日期，或首页 schema 的 default_date（混排按它过滤）
    let active_day = active_day_input
        .clone()
        .unwrap_or_else(|| page.default_date.clone());

    // 混排按展示日过滤（他家 UI 同款口径）；服务端已按日返回时过滤是空操作
    let mut items: Vec<CalendarItem> = page
        .items
        .iter()
        .filter(|i| beijing_date(i.publish_time) == active_day)
        .cloned()
        .collect();
    // 过滤后为空但原始非空（首页未选日）：未定档条目 publish_time 解析不了
    // 会全被滤掉——保留原始顺序兜底；显式选日的空页交给前端自动续拉
    if items.is_empty() && !page.items.is_empty() && active_day_input.is_none() {
        items = page.items.clone();
    }

    Ok(CalendarPage {
        dates: page.dates.clone(),
        default_date: page.default_date.clone(),
        items,
        has_more: page.has_more,
        next_offset: page.next_offset,
        session_id: page.session_id.clone(),
        online_total: page.online_total,
        offline_total: page.offline_total,
        online_unread_count_text: page.online_unread_count_text.clone(),
    })
}

/// 拉一页日历（subscribe/list 日历形态）。
async fn fetch_calendar_page(q: &[(String, String)], env: &ApiEnv) -> AppResult<CalendarPage> {
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析上新日历失败: {e}")))?;
    check_code(&value)?;
    let page = parse_calendar(value.get("data"))?;
    log::info!(
        "[Calendar] q={q:?} -> items={} has_more={} next={} first={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset,
        page.items.first().map(|i| (&i.title, i.is_online))
    );
    Ok(page)
}

/// 解析 subscribe/list 日历形态的 data 节点。
pub(super) fn parse_calendar(data: Option<&Value>) -> AppResult<CalendarPage> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;

    let mut items = Vec::new();
    for raw in data
        .get("subscribe_items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(item) = parse_calendar_item(raw) {
            items.push(item);
        }
    }

    let cal = data.get("calendar_schema");
    Ok(CalendarPage {
        items,
        dates: cal
            .and_then(|c| c.get("date_list"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        default_date: cal
            .and_then(|c| c.get("default_date"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        online_total: data
            .get("online_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        offline_total: data
            .get("offline_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        session_id: data
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        online_unread_count_text: data
            .get("online_unread_count_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 一条日历条目，两种 schema 都吃：嵌套形态（匿名请求，`subscribe_data.*`）与
/// 扁平形态（带 install_id Cookie 或 target_date，字段直接在条目上，无 vid）。
fn parse_calendar_item(raw: &Value) -> Option<CalendarItem> {
    if let Some(sd) = raw.get("subscribe_data") {
        let series_id = sd
            .get("series_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())?;
        return Some(CalendarItem {
            series_id: series_id.to_string(),
            title: str_field(sd, "title"),
            cover: str_field(sd, "cover"),
            vid: str_field(sd, "vid"),
            score: num_field(sd, "score"),
            play_cnt: int_field(sd, "play_cnt"),
            episode_cnt: int_field(sd, "episode_cnt").max(0) as u32,
            description: str_field(sd, "video_desc"),
            category: str_field(raw, "category"),
            rec_tags: string_array(raw, "rec_tags"),
            publish_time: int_field(raw, "schedule_publish_time") * 1000,
            is_online: raw
                .get("is_online")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_subscribed: raw
                .get("has_subscribed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let series_id = raw
        .get("item_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    Some(CalendarItem {
        series_id: series_id.to_string(),
        title: str_field(raw, "name"),
        cover: str_field(raw, "cover"),
        vid: String::new(),
        score: 0.0,
        play_cnt: 0,
        episode_cnt: 0,
        description: str_field(raw, "item_desc"),
        category: str_field(raw, "category"),
        rec_tags: string_array(raw, "sub_title_list"),
        publish_time: int_field(raw, "schedule_publish_time") * 1000,
        is_online: raw
            .get("is_online")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_subscribed: raw
            .get("has_subscribed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `[{content: "..."}, ...]` 形态的标签数组 → 字符串数组。
fn string_array(node: &Value, key: &str) -> Vec<String> {
    node.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| t.get("content").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
