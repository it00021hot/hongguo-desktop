//! “新剧预约”发现列表（不是账号的“我的预约”）。

use hongguo_common::{AppError, AppResult, utils::json::str_field};
use serde_json::Value;

use super::{
    ApiEnv, LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, api_call_reading, check_code, parse_calendar,
};
use crate::calendar::CalendarItem;
use std::collections::HashMap;

use super::selectors::{SelectorRow, SelectorSelection, encode_panel_selection, parse_rows};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpcomingAudience {
    #[default]
    All,
    Male,
    Female,
}

impl UpcomingAudience {
    fn gender_type(self) -> &'static str {
        // Official GetUserSubscribeListRequest Gender enum.
        match self {
            Self::All => "2",
            Self::Male => "1",
            Self::Female => "0",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpcomingFilters {
    Legacy(UpcomingAudience),
    Selectors {
        selected: SelectorSelection,
        panel_keys: Vec<String>,
    },
}

impl Default for UpcomingFilters {
    fn default() -> Self {
        Self::Legacy(UpcomingAudience::All)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpcomingItem {
    pub show: CalendarItem,
    pub anticipation: String,
    pub expected_release: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpcomingPage {
    pub items: Vec<UpcomingItem>,
    pub has_more: bool,
    pub next_offset: i64,
    pub session_id: String,
}

pub async fn fetch_upcoming_panel(env: &ApiEnv) -> AppResult<Vec<SelectorRow>> {
    let query = modern_query(0, None, None, true);
    let response = request(&query, env).await?;
    Ok(parse_rows(&response))
}

pub async fn fetch_upcoming(
    filters: &UpcomingFilters,
    offset: i64,
    session_id: Option<&str>,
    env: &ApiEnv,
) -> AppResult<UpcomingPage> {
    let query = match filters {
        UpcomingFilters::Legacy(audience) => legacy_query(*audience, offset, session_id),
        UpcomingFilters::Selectors {
            selected,
            panel_keys,
        } => modern_query(
            offset,
            session_id,
            encode_panel_selection(selected, panel_keys)?,
            false,
        ),
    };
    let response = request(&query, env).await?;
    parse_upcoming(response.get("data"))
}

async fn request(query: &[(String, String)], env: &ApiEnv) -> AppResult<Value> {
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, query, env).await?;
    let response: Value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Media(format!("解析新剧预约失败: {error}")))?;
    check_code(&response)?;
    Ok(response)
}

fn legacy_query(
    audience: UpcomingAudience,
    offset: i64,
    session_id: Option<&str>,
) -> Vec<(String, String)> {
    let mut params = [
        ("active_panel", "2"),
        ("gender_type", audience.gender_type()),
        ("tab_style", "2"),
        ("tab_type", "2"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect::<Vec<(String, String)>>();
    if offset > 0 {
        params.push(("offset".into(), offset.to_string()));
        params.push(("subscribe_offset".into(), offset.to_string()));
        if let Some(session_id) = session_id.filter(|id| !id.is_empty()) {
            params.push(("session_id".into(), session_id.into()));
        }
    }
    params
}

fn modern_query(
    offset: i64,
    session_id: Option<&str>,
    selected: Option<String>,
    panel: bool,
) -> Vec<(String, String)> {
    // Official Wg -> Y/J: the panel and selected lists use version 1, size 10,
    // and new requests use an empty browsing session. Only after panel rows exist
    // can a user request selected lists with the server-provided choice IDs.
    let mut params = vec![
        ("tab_type".into(), "2".into()),
        ("limit".into(), "10".into()),
        ("offset".into(), offset.to_string()),
        ("session_id".into(), session_id.unwrap_or_default().into()),
        ("version".into(), "1".into()),
    ];
    if panel {
        params.push(("req_type".into(), "panel_only".into()));
    }
    if let Some(selected) = selected {
        params.push(("select_items".into(), selected));
    }
    params
}

fn parse_upcoming(data: Option<&Value>) -> AppResult<UpcomingPage> {
    let page = parse_calendar(data)?;
    let decorations: HashMap<_, _> = data
        .and_then(|data| data.get("subscribe_items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|raw| {
            let id = raw
                .pointer("/subscribe_data/series_id")
                .or_else(|| raw.get("item_id"))?
                .as_str()?;
            Some((id, raw))
        })
        .collect();
    let items = page
        .items
        .into_iter()
        .map(|mut show| {
            let mut anticipation = String::new();
            let mut expected_release = String::new();
            if let Some(raw) = decorations.get(show.series_id.as_str()) {
                if show.cover.is_empty() {
                    show.cover = str_field(raw, "cover");
                }
                if show.title.trim().is_empty() {
                    show.title = str_field(raw, "name");
                }
                if show.description.is_empty() {
                    show.description = str_field(raw, "item_desc");
                }
                anticipation = str_field(raw, "expected_rec_text");
                expected_release = str_field(raw, "expected_publish_time");
            }
            UpcomingItem {
                show,
                anticipation,
                expected_release,
            }
        })
        .collect();
    Ok(UpcomingPage {
        items,
        has_more: page.has_more,
        next_offset: page.next_offset,
        session_id: page.session_id,
    })
}
