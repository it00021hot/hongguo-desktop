//! Official calendar panel and filtered list (separate requests).

use super::{ApiEnv, CalendarPage, SUBSCRIBE_LIST_PATH, parse_calendar};
use crate::{
    client::api_call_reading,
    danmaku::LQ_API_ORIGIN,
    reservation::selectors::{SelectorRow, SelectorSelection, encode_selection, parse_rows},
};
use hongguo_common::{AppError, AppResult, utils::json::check_code};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalendarPanel {
    pub dates: Vec<String>,
    pub default_date: String,
    pub row: Option<SelectorRow>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalendarCursor {
    pub offset: i64,
    pub subscribe_offset: Option<i64>,
    pub session: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalendarFilteredPage {
    pub page: CalendarPage,
    pub cursor: CalendarCursor,
}

pub async fn fetch_calendar_panel(env: &ApiEnv) -> AppResult<CalendarPanel> {
    let response = get(
        vec![
            ("offset", "0".into()),
            ("session_id", String::new()),
            ("req_type", "panel_only".into()),
            ("need_calendar_schema", "true".into()),
        ],
        env,
    )
    .await?;
    let data = response.get("data");
    let page = parse_calendar(data)?;
    let default_date = if page.default_date.is_empty() {
        data.and_then(|data| data.pointer("/calendar_schema/current_date"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    } else {
        page.default_date
    };
    if default_date.is_empty() || page.dates.is_empty() {
        return Err(AppError::Media("上新日历缺少日期面板".into()));
    }
    Ok(CalendarPanel {
        dates: page.dates,
        default_date,
        row: parse_rows(&response).into_iter().next(),
    })
}

pub async fn fetch_filtered_calendar(
    date: &str,
    selection: &SelectorSelection,
    cursor: Option<&CalendarCursor>,
    env: &ApiEnv,
) -> AppResult<CalendarFilteredPage> {
    let mut query = vec![
        ("target_date", date.into()),
        (
            "offset",
            cursor.map_or(0, |cursor| cursor.offset).to_string(),
        ),
        (
            "session_id",
            cursor.map_or_else(String::new, |cursor| cursor.session.clone()),
        ),
        ("need_calendar_schema", "false".into()),
    ];
    if cursor.is_none() && !selection.is_empty() {
        query.push(("swipe_type", "3".into()));
    }
    if let Some(offset) = cursor.and_then(|cursor| cursor.subscribe_offset) {
        query.push(("subscribe_offset", offset.to_string()));
    }
    if let Some(selected) = encode_selection(selection)? {
        query.push(("select_items", selected));
    }
    let response = get(query, env).await?;
    let data = response.get("data");
    let page = parse_calendar(data)?;
    let subscribe_offset = data
        .and_then(|data| data.pointer("/calendar_loc_info/subscribe_offset"))
        .and_then(Value::as_i64);
    let next = CalendarCursor {
        offset: page.next_offset,
        subscribe_offset,
        session: page.session_id.clone(),
    };
    Ok(CalendarFilteredPage { page, cursor: next })
}

async fn get(query: Vec<(&str, String)>, env: &ApiEnv) -> AppResult<Value> {
    let mut query: Vec<_> = query
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    query.extend([
        ("tab_type".into(), "5".into()),
        ("version".into(), "0".into()),
    ]);
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &query, env).await?;
    let response: Value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Media(format!("解析上新日历失败: {error}")))?;
    check_code(&response)?;
    Ok(response)
}
