//! 找剧搜索接口（2026-10-03 抓包锁定）。
//!
//! `GET /reading/bookapi/search/tab/v`：
//! - `query` + `tab_type=11`（综合 tab）+ `count=100` + `offset`；
//! - 翻页要带首响应的 `search_id` 与 `passback=offset`；
//! - 响应 `search_tabs[]`（综合=11/剧集=29/视频=30/讨论=31/小说=1/用户=27/听书=2），
//!   综合 tab `data[].video_data[]` 是剧集条目，`has_more/next_offset` 控制翻页。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{ApiEnv, api_call_full_with_headers, reading_headers};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, str_field};
use crate::error::{AppError, AppResult};

pub const SEARCH_TAB_PATH: &str = "/reading/bookapi/search/tab/v";

/// 搜索联想（2026-10-07 抓 hgplayer 1.1.6 锁定）。
pub const SUGGEST_PATH: &str = "/reading/bookapi/search/suggest/v";

/// 联想词的一个渲染片段（hgplayer 同款 TextPart：按命中位切开，
/// hl=true 的片段前端上高亮色）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestPart {
    pub text: String,
    pub hl: bool,
}

/// 一条搜索联想（query_result_v2 形态）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestItem {
    /// 联想词（= 剧名；v2 的 name）
    pub word: String,
    /// 命中高亮切片（search_high_light.high_light_position 切 name 所得；
    /// 无高亮信息为空，前端整体按普通文本渲染）
    #[serde(default)]
    pub parts: Vec<SuggestPart>,
    /// 对应剧集 id（= keyword；无 video_data 的纯词联想为空串，
    /// 前端回落为「以该词发起搜索」）
    #[serde(default)]
    pub series_id: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub cover: String,
    /// 摘要行（「第1季·玄幻·4105万热度」，v2 的 sug_abstract）
    #[serde(default, rename = "abstract")]
    pub abstract_text: String,
}

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

/// 搜索联想：输入 2~3 个字即返回相关剧集（hgplayer 1.1.6 找剧搜索框同款）。
///
/// 业务参数全部在 query（逐值对齐 2026-10-07 抓包）：`q` 是查询词——
/// 注意不是 search/tab 的 `query`。响应 `data.query_result_v2[]`。
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

/// query_result_v2 → 联想条目。series_id 优先 video_data（带封面/vid 可直拨），
/// 没有时退 keyword（纯词联想）。两者都缺的废条目跳过。
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
            None => (str_field(raw, "keyword"), String::new(), String::new()),
        };
        items.push(SuggestItem {
            parts: highlight_parts(&word, raw.pointer("/search_high_light/high_light_position")),
            word,
            series_id,
            vid,
            cover,
            abstract_text: str_field(raw, "sug_abstract"),
        });
    }
    Ok(items)
}

/// high_light_position（`[[start,len]]`，按 unicode 字符计，抓包实证
/// 「云渺：不死帝师」的「不死」= [3,2]）→ 把 word 切成 hl 片段。
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
        has_more: tab
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: tab.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
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

    #[test]
    fn parses_suggest_v2_items() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": {
                "query_key": "不死",
                "query_result_v2": [
                    { "name": "不死帝师", "keyword": "7644", "sug_abstract": "第1季·玄幻·4105万热度",
                      "search_high_light": {
                          "text": "不死帝师",
                          "rich_text": "<em>不死</em>帝师",
                          "high_light_position": [[0, 2]]
                      },
                      "video_data": { "series_id": "7644", "vid": "99", "cover": "https://x/heic",
                                      "title": "不死帝师" } },
                    { "name": "纯词联想", "keyword": "纯词联想" },
                    { "name": "", "keyword": "废条目" }
                ]
            }
        });
        let items = parse_suggest(&v).unwrap();
        assert_eq!(items.len(), 2, "空 name 跳过");
        assert_eq!(items[0].series_id, "7644");
        assert_eq!(items[0].vid, "99");
        assert_eq!(items[0].abstract_text, "第1季·玄幻·4105万热度");
        assert_eq!(
            items[0].parts,
            vec![
                SuggestPart {
                    text: "不死".into(),
                    hl: true
                },
                SuggestPart {
                    text: "帝师".into(),
                    hl: false
                },
            ],
            "命中位切成 hl 片段"
        );
        assert_eq!(items[1].series_id, "纯词联想", "无 video_data 退 keyword");
        assert_eq!(items[1].cover, "");
        assert!(items[1].parts.is_empty(), "无高亮信息 parts 空");
    }

    #[test]
    fn highlight_parts_tolerates_bad_positions() {
        // 非前缀命中 + 乱序/越界钳制（「云渺：不死帝师」抓包 = [3,2]）
        let pos: Value = serde_json::json!([[3, 2], [99, 5], [1, 0], [-1, 2], [2, 1]]);
        let parts = highlight_parts("云渺：不死帝师", Some(&pos));
        assert_eq!(
            parts,
            vec![
                SuggestPart {
                    text: "云渺".into(),
                    hl: false
                },
                SuggestPart {
                    text: "：".into(),
                    hl: true
                },
                SuggestPart {
                    text: "不死".into(),
                    hl: true
                },
                SuggestPart {
                    text: "帝师".into(),
                    hl: false
                },
            ],
            "重叠/越界/非法位丢弃，其余按序切片"
        );
        assert!(highlight_parts("无高亮", None).is_empty());
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 搜索直连：真实关键词 + 翻页衔接。
    ///
    /// 2026-10-04：服务端按 install_id 风控（旧 id 0 字节拒），生产环境
    /// 静态档案 + 匿名 Cookie 即可过——本用例与生产 api_env 同构。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_search_and_paginate() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
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

    /// 「道爷不悟道，杀妖涨修为，第一季」直连复现（用户报搜不出/加载失败）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_search_daoye() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        for q in [
            "道爷不悟道，杀妖涨修为，第一季",
            "道爷不悟道",
            "道爷不悟道，杀妖涨修为",
        ] {
            match search_series(q, 0, "", &env).await {
                Ok(p) => {
                    println!(
                        "[daoye] q={q} → {} 条 has_more={}",
                        p.items.len(),
                        p.has_more
                    );
                    for it in p.items.iter().take(3) {
                        println!("[daoye]   {} {}", it.series_id, it.title);
                    }
                }
                Err(e) => println!("[daoye] q={q} → 失败: {e}"),
            }
        }
    }

    /// 联想直连：真实关键词（2026-10-07 抓包形态对齐后验证）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_search_suggest() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let items = search_suggest("女帝", &env).await.expect("联想");
        println!("[Suggest] 命中 {} 条", items.len());
        for it in items.iter().take(3) {
            println!(
                "[Suggest] {} → series={} abstract={}",
                it.word, it.series_id, it.abstract_text
            );
        }
        assert!(!items.is_empty(), "「女帝」至少应有联想词");
        assert!(
            items.iter().any(|i| !i.series_id.is_empty()),
            "至少一条带 series_id（可直拨播放）"
        );
    }
}
