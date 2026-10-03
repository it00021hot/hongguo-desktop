//! 发现类接口：推荐信息流（榜单落地页）。
//!
//! 实测口径（2026-10 直连探测锁定）：
//! - 路径 `/reading/distribution/category/landpage/v1/`，**必须 POST**（GET 404）；
//! - body 的 `biz_param` 内容被服务端忽略——**业务参数全在 URL query**
//!   （`offset` 进 query 翻页生效、进 body 无效，同一批内容反复返回）；
//! - 响应信封 `{"code": 0, "data": {video_data[], next_offset, has_more, session_id}}`；
//! - `video_data[].category_schema` 是**二次序列化的 JSON 字符串**（题材标签表）。
//!
//! 预约日历（`search/uncover_subscribe`）与书城 tab（`bookmall/tab`）的
//! 必填参数尚未破解（POST 报 100103 PARAM_INVALID，GET 404），探测用例
//! 留在文末 `probe` 模块，待抓包补参后转正。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{api_call_full, ApiEnv};
use crate::error::{AppError, AppResult};
use crate::signer::API_ORIGIN;

/// 推荐信息流落地页。
pub const LANDPAGE_PATH: &str = "/reading/distribution/category/landpage/v1/";

/// 信息流的一条剧集卡片。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub series_id: String,
    pub title: String,
    /// 竖版封面（卡片网格用）
    #[serde(default)]
    pub cover: String,
    /// 横版封面（详情头部可选用）
    #[serde(default)]
    pub horiz_cover: String,
    /// 当前推荐位的 vid（直接起播用）
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 播放量（热榜排序键）
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub comment_count: i64,
    #[serde(default)]
    pub score: f64,
    /// 题材标签（来自 category_schema 字符串的二次解析）
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 一页信息流。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    pub next_offset: i64,
    pub has_more: bool,
    #[serde(default)]
    pub session_id: String,
}

/// 拉一页推荐信息流。
///
/// `offset` 传 0 取首页；翻页用上一页返回的 `next_offset`。
pub async fn fetch_feed(offset: i64, env: &ApiEnv) -> AppResult<FeedPage> {
    // 业务参数进 query（实测唯一生效的位置）；body 只是 POST 形状占位
    let biz_query = vec![("offset".to_string(), offset.to_string())];
    let body = serde_json::to_vec(&serde_json::json!({ "biz_param": {} }))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &biz_query, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    parse_feed(value.get("data"))
}

/// 业务错误码检查。
fn check_code(value: &Value) -> AppResult<()> {
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(AppError::Media(format!("接口返回 {code}: {msg}")));
        }
    }
    Ok(())
}

/// 解析 data 节点为 FeedPage。
fn parse_feed(data: Option<&Value>) -> AppResult<FeedPage> {
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

/// category_schema 是 JSON 字符串：`[{"category_id":..,"name":"逆袭",...}]`，
/// 取 name 做题材标签。解析失败给空表（标签是展示增强，不值得报错）。
fn parse_tags(schema: Option<&Value>) -> Vec<String> {
    let Some(s) = schema.and_then(Value::as_str) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(s) else {
        return Vec::new();
    };
    parsed
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.get("name").and_then(Value::as_str))
                .take(4)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 数字字段容忍字符串形态（平台对大数偶发走字符串）。
fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn num_field(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
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

    #[test]
    fn code_check_rejects_nonzero() {
        let v: Value = serde_json::json!({ "code": 100103, "message": "PARAM_INVALID" });
        assert!(check_code(&v).is_err());
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    /// 预约页（新剧日历）。参数未破解，仅探测用。
    const UNCOVER_SUBSCRIBE_PATH: &str = "/reading/bookapi/search/uncover_subscribe/v1/";

    /// 真实接口探测（不进常规测试套件）：
    /// `cargo test probe_ -- --ignored --nocapture`
    fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(crate::domain::model::ProxyConfig::default())
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_landpage_pagination() {
        let env = anon_env();
        let p1 = fetch_feed(0, &env).await.expect("第一页");
        println!(
            "[feed] page1: {} items, has_more={}, next_offset={}",
            p1.items.len(),
            p1.has_more,
            p1.next_offset
        );
        let p2 = fetch_feed(p1.next_offset, &env)
            .await
            .expect("第二页（query offset 翻页）");
        let ids1: Vec<&str> = p1.items.iter().map(|i| i.series_id.as_str()).collect();
        let ids2: Vec<&str> = p2.items.iter().map(|i| i.series_id.as_str()).collect();
        let overlap = ids1.iter().filter(|i| ids2.contains(i)).count();
        println!(
            "[feed] page2: {} items, overlap={overlap}/{}, sample={:?}",
            p2.items.len(),
            p1.items.len(),
            &ids2[..ids2.len().min(3)]
        );
        assert!(!p1.items.is_empty());
        assert!(overlap < p1.items.len(), "翻页必须换内容，不能重复同一页");
    }

    /// 弹幕/评论列表参数探测：code==0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_comment_list() {
        let env = anon_env();
        // 先从信息流拿一个真实 vid 做锚点
        let feed = fetch_feed(0, &env).await.expect("信息流");
        let vid = feed
            .items
            .iter()
            .find(|i| !i.vid.is_empty())
            .map(|i| i.vid.clone())
            .expect("信息流里应有 vid");
        println!("[danmaku] anchor vid={vid}");

        let combos = [
            ("https://lifeapi5-normal-lq.fqnovel.com", "/novel/commentapi/comment/list/"),
            ("https://lifeapi5-normal-lq.fqnovel.com", "/novel/commentapi/comment/list/v1/"),
            ("https://novel.snssdk.com", "/novel/commentapi/comment/list/v1/"),
        ];
        for (origin, path) in combos {
            for (tag, biz) in [
                ("group", serde_json::json!({ "group_id": vid, "count": 20, "offset": 0 })),
                (
                    "group-item",
                    serde_json::json!({ "group_id": vid, "item_id": vid, "count": 20 }),
                ),
                (
                    "group-ct2",
                    serde_json::json!({ "group_id": vid, "count": 20, "comment_type": 2 }),
                ),
            ] {
                let body = serde_json::to_vec(&serde_json::json!({ "biz_param": biz })).unwrap();
                match api_call_full(origin, path, Some(body), &[], &env).await {
                    Ok(bytes) => {
                        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                        println!(
                            "[danmaku/{tag}@{origin}{}] {}",
                            path,
                            serde_json::to_string(&v).unwrap_or_default().chars().take(280).collect::<String>()
                        );
                    }
                    Err(e) => println!("[danmaku/{tag}@{path}] ERR {e}"),
                }
            }
        }
    }

    /// 预约日历参数破解的试验场：每轮改候选参数跑一次，code==0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_uncover_subscribe() {
        let env = anon_env();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let candidates: Vec<(&str, Value)> = vec![
            ("need-calendar", serde_json::json!({ "need_calendar_schema": true })),
            ("date", serde_json::json!({ "date": today })),
        ];
        for (tag, biz) in candidates {
            let body =
                serde_json::to_vec(&serde_json::json!({ "biz_param": biz })).unwrap();
            match api_call_full(API_ORIGIN, UNCOVER_SUBSCRIBE_PATH, Some(body), &[], &env).await {
                Ok(bytes) => {
                    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    println!(
                        "[uncover/{tag}] {}",
                        serde_json::to_string(&v).unwrap_or_default().chars().take(300).collect::<String>()
                    );
                }
                Err(e) => println!("[uncover/{tag}] ERR {e}"),
            }
        }
    }
}
