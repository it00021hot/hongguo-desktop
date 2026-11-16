//! 找剧搜索接口：`GET /reading/bookapi/search/tab/v`，`query` + `tab_type=11`
//!（综合 tab）+ `count=100` + `offset`，翻页带首响应的 `search_id` 与 `passback=offset`。
//! 端点与 tab 枚举详见 docs/hongguo-api-endpoints.md。

mod model;

pub use model::{SearchPage, SearchResult, SuggestItem, SuggestPart};

use serde_json::Value;

use super::client::{ApiEnv, api_call_full_with_headers, reading_headers};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, str_field};
use hongguo_common::{AppError, AppResult};

pub const SEARCH_TAB_PATH: &str = "/reading/bookapi/search/tab/v";

/// 搜索联想。
pub const SUGGEST_PATH: &str = "/reading/bookapi/search/suggest/v";

/// 翻页参数检查：offset>0 必须带 search_id（首页发放的会话 id）。
fn validate_pagination(offset: i64, search_id: &str) -> AppResult<()> {
    if offset > 0 && search_id.is_empty() {
        return Err(AppError::Media("翻页缺少 search_id".into()));
    }
    Ok(())
}

/// 搜索综合 tab。首页 `offset=0, search_id=""`；翻页传上一页返回值。
pub async fn search_series(
    query: &str,
    offset: i64,
    search_id: &str,
    env: &ApiEnv,
) -> AppResult<SearchPage> {
    validate_pagination(offset, search_id)?;
    let mut q: Vec<(String, String)> = vec![
        ("query".into(), query.to_string()),
        ("tab_type".into(), "11".into()),
        ("count".into(), "100".into()),
        ("offset".into(), offset.to_string()),
        ("use_correct".into(), "true".into()),
        ("bookshelf_search_plan".into(), "4".into()),
    ];
    if offset > 0 {
        q.push(("passback".into(), offset.to_string()));
        q.push(("search_id".into(), search_id.to_string()));
    }
    let bytes = api_call_full_with_headers(
        LQ_API_ORIGIN,
        SEARCH_TAB_PATH,
        None,
        &q,
        &reading_headers(),
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析搜索失败: {e}")))?;
    check_code(&value)?;
    parse_search(&value)
}

/// 搜索联想：输入 2~3 个字即返回相关剧集（找剧搜索框同款行为）。
/// 业务参数全在 query，查询词键是 `q`（不是 search/tab 的 `query`）；响应 `data.query_result_v2[]`。
pub async fn search_suggest(q: &str, env: &ApiEnv) -> AppResult<Vec<SuggestItem>> {
    let query: Vec<(String, String)> = vec![
        ("q".into(), q.to_string()),
        ("bookshelf_search_plan".into(), "4".into()),
        ("bookstore_tab".into(), "16".into()),
        ("count".into(), "0".into()),
        ("need_personal_recommend".into(), "1".into()),
        ("need_preload".into(), "true".into()),
        ("search_source".into(), "1".into()),
        ("tab_name".into(), "feed".into()),
    ];
    let bytes = api_call_full_with_headers(
        LQ_API_ORIGIN,
        SUGGEST_PATH,
        None,
        &query,
        &reading_headers(),
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析联想失败: {e}")))?;
    check_code(&value)?;
    parse_suggest(&value)
}

/// query_result_v2 → 联想条目；仅 video_data 提供剧集标识，纯词联想保留搜索语义。
fn parse_suggest(value: &Value) -> AppResult<Vec<SuggestItem>> {
    let data = value
        .get("data")
        .ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut items = Vec::new();
    for raw in data
        .get("query_result_v2")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let word = str_field(raw, "name");
        if word.is_empty() {
            continue;
        }
        let (series_id, vid, cover) = match raw.get("video_data") {
            Some(vd) => (
                str_field(vd, "series_id"),
                str_field(vd, "vid"),
                str_field(vd, "cover"),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        let (book_title, book_cover, book_score) = match raw.get("book_data") {
            Some(b) => (
                str_field(b, "title"),
                str_field(b, "cover"),
                str_field(b, "score"),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        items.push(SuggestItem {
            parts: highlight_parts(&word, raw.pointer("/search_high_light/high_light_position")),
            word,
            series_id,
            vid,
            cover,
            abstract_text: str_field(raw, "sug_abstract"),
            book_title,
            book_cover,
            book_score,
        });
    }
    Ok(items)
}

/// high_light_position（`[[start,len]]`，按 unicode 字符计）→ 把 word 切成 hl 片段。
/// 位置越界/乱序按钳制+排序容错，切不出来的退空（前端整体普通渲染）。
fn highlight_parts(word: &str, positions: Option<&Value>) -> Vec<SuggestPart> {
    let chars: Vec<char> = word.chars().collect();
    let mut ranges: Vec<(usize, usize)> = positions
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let arr = p.as_array()?;
            let start = arr.first()?.as_i64()?;
            let len = arr.get(1)?.as_i64()?;
            if start < 0 || len <= 0 {
                return None;
            }
            Some((start as usize, len as usize))
        })
        .collect();
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.sort_unstable();
    let mut parts = Vec::new();
    let mut cursor = 0;
    for (start, len) in ranges {
        let start = start.min(chars.len());
        let end = start.saturating_add(len).min(chars.len());
        if start < cursor || end <= start {
            continue;
        }
        if start > cursor {
            parts.push(SuggestPart {
                text: chars[cursor..start].iter().collect(),
                hl: false,
            });
        }
        parts.push(SuggestPart {
            text: chars[start..end].iter().collect(),
            hl: true,
        });
        cursor = end;
    }
    if cursor < chars.len() {
        parts.push(SuggestPart {
            text: chars[cursor..].iter().collect(),
            hl: false,
        });
    }
    parts
}

/// 从 `search_tabs` 里取综合 tab（tab_type=11）解析。
fn parse_search(value: &Value) -> AppResult<SearchPage> {
    let tabs = value
        .get("search_tabs")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("响应缺少 search_tabs".into()))?;
    let tab = tabs
        .iter()
        .find(|t| t.get("tab_type").and_then(Value::as_i64) == Some(11))
        .ok_or_else(|| AppError::Media("响应缺少综合 tab".into()))?;

    let mut items = Vec::new();
    for cell in tab
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // cell 的 video_data 可能是数组也可能是单对象，统一平铺
        let vds: Vec<Value> = match cell.get("video_data") {
            Some(Value::Array(a)) => a.clone(),
            Some(v @ Value::Object(_)) => vec![v.clone()],
            _ => Vec::new(),
        };
        for raw in &vds {
            let Some(series_id) = raw
                .get("series_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let (tags, heat_text) = parse_sub_titles(raw);
            items.push(SearchResult {
                series_id: series_id.to_string(),
                title: str_field(raw, "title"),
                cover: str_field(raw, "cover"),
                vid: str_field(raw, "vid"),
                sub_title: str_field(raw, "sub_title"),
                score: super::discover::num_field(raw, "score"),
                play_cnt: int_field(raw, "play_cnt"),
                episode_cnt: int_field(raw, "episode_cnt").max(0) as u32,
                description: str_field(raw, "video_desc"),
                tags,
                heat_text,
            });
        }
    }
    Ok(SearchPage {
        has_more: tab
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: tab.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        search_id: str_field(tab, "search_id"),
        items,
    })
}

/// 副标题列表双键名双形态归一：`series_sub_title_list`（JSON 字符串内嵌数组）与
/// `sub_title_list[]`（直数组）两个键都认，谁在场用谁；热度行取含「热度」的那条。
fn parse_sub_titles(raw: &Value) -> (Vec<String>, String) {
    let field = raw
        .get("series_sub_title_list")
        .or_else(|| raw.get("sub_title_list"));
    let entries: Vec<String> = match field {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
        _ => Vec::new(),
    };
    let heat = entries
        .iter()
        .find(|e| e.contains("热度"))
        .cloned()
        .unwrap_or_default();
    (entries, heat)
}
