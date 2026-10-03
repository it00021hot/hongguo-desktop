//! 找剧搜索接口（2026-10-03 抓包锁定）。
//!
//! `GET /reading/bookapi/search/tab/v`：
//! - `query` + `tab_type=11`（综合 tab）+ `count=100` + `offset`；
//! - 翻页要带首响应的 `search_id` 与 `passback=offset`；
//! - 响应 `search_tabs[]`（综合=11/剧集=29/视频=30/讨论=31/小说=1/用户=27/听书=2），
//!   综合 tab `data[].video_data[]` 是剧集条目，`has_more/next_offset` 控制翻页。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{api_call_full_with_headers, ApiEnv};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, str_field};
use crate::error::{AppError, AppResult};

/// search 系要 reading 轻签名头（抓包：无 gorgon/argus/ladon，
/// 多 x-ss-dp + x-reading-request，格式同 commentapi 的 ticket-random）。
fn reading_headers() -> [(String, String); 3] {
    let ticket = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rnd: u32 = rand::random();
    [
        ("x-reading-request".to_string(), format!("{ticket}-{rnd}")),
        ("x-ss-dp".to_string(), "8662".to_string()),
        ("lc".to_string(), "101".to_string()),
    ]
}

pub const SEARCH_TAB_PATH: &str = "/reading/bookapi/search/tab/v";

/// 一条搜索结果（形状同榜单条目，省去榜单专属字段）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub sub_title: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    #[serde(default)]
    pub description: String,
}

/// 一页搜索结果。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<SearchResult>,
    pub has_more: bool,
    pub next_offset: i64,
    /// 首页响应发放的会话 id，翻页时原样带回。
    #[serde(default)]
    pub search_id: String,
}

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
    let bytes =
        api_call_full_with_headers(LQ_API_ORIGIN, SEARCH_TAB_PATH, None, &q, &reading_headers(), env)
            .await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析搜索失败: {e}")))?;
    check_code(&value)?;
    parse_search(&value)
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
            let Some(series_id) =
                raw.get("series_id").and_then(Value::as_str).filter(|s| !s.is_empty())
            else {
                continue;
            };
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
            });
        }
    }
    Ok(SearchPage {
        has_more: tab.get("has_more").and_then(Value::as_bool).unwrap_or(false),
        next_offset: tab
            .get("next_offset")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        search_id: str_field(tab, "search_id"),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_composite_tab_and_skips_empty() {
        let v: Value = serde_json::json!({
            "code": 0,
            "search_tabs": [
                { "tab_type": 29, "title": "剧集", "data": [] },
                { "tab_type": 11, "title": "综合", "has_more": true, "next_offset": 100,
                  "search_id": "####11@x",
                  "data": [
                      { "video_data": [
                          { "series_id": "s1", "title": "剧A", "sub_title": "脑洞·全273集",
                            "score": "8.3", "episode_cnt": 273 },
                          { "series_id": "", "title": "废" }
                      ]},
                      { "video_data": { "series_id": "s2", "title": "单对象形态" } }
                  ] }
            ]
        });
        let page = parse_search(&v).unwrap();
        assert_eq!(page.items.len(), 2, "数组与单对象形态都要收，空 id 跳过");
        assert_eq!(page.items[0].series_id, "s1");
        assert_eq!(page.items[0].score, 8.3);
        assert_eq!(page.items[1].series_id, "s2");
        assert!(page.has_more);
        assert_eq!(page.next_offset, 100);
        assert_eq!(page.search_id, "####11@x");
    }

    #[test]
    fn missing_composite_tab_is_error() {
        let v: Value = serde_json::json!({ "code": 0, "search_tabs": [] });
        assert!(parse_search(&v).is_err());
    }

    #[test]
    fn pagination_requires_search_id() {
        let err = validate_pagination(6, "").unwrap_err();
        assert!(err.to_string().contains("search_id"));
        assert!(validate_pagination(0, "").is_ok(), "首页无需 search_id");
        assert!(validate_pagination(6, "####11@x").is_ok());
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 搜索直连：真实关键词 + 翻页衔接。
    /// 抓包里 search 带 cookie（store-region 等），匿名形态空响应，用 hgplayer 设备档案。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_search_and_paginate() {
        let env = super::super::rank::probe::hg_env(&ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        ));
        let p1 = search_series("丧尸", 0, "", &env).await.expect("首页");
        println!(
            "[search] p1: {} 条, has_more={}, search_id={:?}, #1={:?}",
            p1.items.len(),
            p1.has_more,
            p1.search_id,
            p1.items.first().map(|i| &i.title)
        );
        assert!(!p1.items.is_empty());
        if p1.has_more && !p1.search_id.is_empty() {
            let p2 = search_series("丧尸", p1.next_offset, &p1.search_id, &env)
                .await
                .expect("翻页");
            let ids1: Vec<&str> = p1.items.iter().map(|i| i.series_id.as_str()).collect();
            let ids2: Vec<&str> = p2.items.iter().map(|i| i.series_id.as_str()).collect();
            let overlap = ids2.iter().filter(|i| ids1.contains(i)).count();
            println!("[search] p2: {} 条, overlap={overlap}", p2.items.len());
            assert!(overlap < p2.items.len(), "翻页不能重复同一页");
        }
    }
}
