//! 发现类接口：推荐信息流（榜单落地页）。
//!
//! 实测口径（2026-10 直连探测锁定）：
//! - 路径 `/reading/distribution/category/landpage/v1/`，**必须 POST**（GET 404）；
//! - **官方 body 协议**（2026-10-06 手机端抓包对齐）：业务参数全部在
//!   POST body——`client_req_type/limit/offset/req_type:"only_content"/
//!   select_items{genre:[...]}`。`genre` 取值：真人剧=short_play、
//!   漫剧=comic_series、AI剧=ai_series（selector 面板的「体裁」维度，
//!   服务端过滤，条目自带的 content_type 数字不可靠：1 里混着动漫）；
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
    /// 内容类型：1=真人剧，1004=漫剧（推荐流「按类型刷」的过滤键）
    #[serde(default)]
    pub content_type: i64,
}

/// selector「体裁」维度（genre）的取值——官方推荐流的内容类型过滤键。
pub const GENRE_HUMAN: &str = "short_play";
pub const GENRE_COMIC: &str = "comic_series";
pub const GENRE_AI: &str = "ai_series";

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

/// 拉一页推荐信息流（官方 body 协议，genre 服务端过滤）。
///
/// `offset` 传 0 取首页；翻页用上一页返回的 `next_offset`。
/// `genre`：`Some(GENRE_COMIC)` 只回漫剧、`Some(GENRE_HUMAN)` 只回真人剧，
/// `None` 全部（官方「全部体裁」）。
pub async fn fetch_feed(offset: i64, genre: Option<&str>, env: &ApiEnv) -> AppResult<FeedPage> {
    let genre_ids: Vec<&str> = genre.into_iter().collect::<Vec<_>>();
    let body = serde_json::to_vec(&serde_json::json!({
        "client_req_type": 3,
        "filter_ids": "",
        "limit": 18,
        "need_selector_panel": false,
        "offset": offset,
        "req_scene": "default",
        "req_type": "only_content",
        "select_items": {
            "category_dim_epoch": [],
            "category_dim_role": [],
            "category_dim_theme": [],
            "gender": [],
            "genre": genre_ids,
            "online_time": [],
            "sort": []
        },
        "session_id": ""
    }))
    .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    parse_feed(value.get("data"))
}

/// 业务错误码检查。
pub(super) fn check_code(value: &Value) -> AppResult<()> {
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

/// category_schema 是 JSON 字符串：`[{"category_id":..,"name":"逆袭",...}]`，
/// 取 name 做题材标签。解析失败给空表（标签是展示增强，不值得报错）。
pub(super) fn parse_tags(schema: Option<&Value>) -> Vec<String> {
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

pub(super) fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 数字字段容忍字符串形态（平台对大数偶发走字符串）。
pub(super) fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub(super) fn num_field(v: &Value, key: &str) -> f64 {
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

    /// 真实接口探测（不进常规测试套件）：
    /// `cargo test probe_ -- --ignored --nocapture`
    fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(crate::domain::model::ProxyConfig::default())
    }

    /// 推荐流内容分类参数探测：landpage 是否支持按内容类型（真人/漫剧/AI）
    /// 过滤。返回条目的类型标记字段一并盘点——这是「推荐全是真人剧」
    /// 问题的修法验证（hgplayer 走 multi_video_model 关联推荐，匿名设备
    /// 拿不到个性化，landpage 分类过滤是可控替代）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_content_type_params() {
        let env = anon_env();
        let try_case = |extra: Vec<(&str, String)>| -> Vec<(String, String)> {
            let mut q = vec![("offset".to_string(), "0".to_string())];
            for (k, v) in extra {
                q.push((k.to_string(), v));
            }
            q
        };
        let cases: Vec<(&str, Vec<(&str, String)>)> = vec![
            ("baseline", vec![]),
            (
                "category=comic_series_rank",
                vec![("req_rank_category_id", "comic_series_rank".into())],
            ),
            (
                "category=human",
                vec![("req_rank_category_id", "human".into())],
            ),
            (
                "category numeric=2",
                vec![("req_rank_category_id", "2".into())],
            ),
            (
                "video_type prefs comic",
                vec![(
                    "video_type_preferences_str",
                    "[{\"video_type\":\"comic_series_rank\"}]".into(),
                )],
            ),
            (
                "tab_type comic",
                vec![("tab_type", "32".into()), ("tab_index", "3".into())],
            ),
        ];
        for (name, extra) in cases {
            let body = serde_json::to_vec(&serde_json::json!({ "biz_param": {} })).unwrap();
            let bytes = match super::super::client::api_call_full(
                API_ORIGIN,
                LANDPAGE_PATH,
                Some(body),
                &try_case(extra),
                &env,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    println!("[feed-type] {name}: ERR {e}");
                    continue;
                }
            };
            let v: Value = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(e) => {
                    println!("[feed-type] {name}: 非JSON({})", e);
                    continue;
                }
            };
            let code = v.get("code").and_then(Value::as_i64);
            let items = v
                .pointer("/data/video_data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            // 类型标记候选字段：条目级 video_type / sub_title_list / tags
            let mut type_hits: Vec<String> = Vec::new();
            for it in items.iter().take(8) {
                for key in ["video_type", "category", "sub_title_list"] {
                    if let Some(m) = it.get(key) {
                        let s = match m {
                            Value::String(s) => s.clone(),
                            other => serde_json::to_string(other).unwrap_or_default(),
                        };
                        if s.len() < 120 && !type_hits.contains(&s) {
                            type_hits.push(s);
                        }
                    }
                }
            }
            let first_keys: Vec<String> = items
                .first()
                .and_then(|i| i.as_object().map(|o| o.keys().cloned().collect()))
                .unwrap_or_default();
            let typeish: Vec<&String> = first_keys
                .iter()
                .filter(|k| k.contains("type") || k.contains("category"))
                .collect();
            println!(
                "[feed-type] {name}: code={code:?} items={} type_fields={typeish:?} markers={:?}",
                items.len(),
                &type_hits[..type_hits.len().min(6)]
            );
        }
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_landpage_pagination() {
        let env = anon_env();
        let p1 = fetch_feed(0, None, &env).await.expect("第一页");
        println!(
            "[feed] page1: {} items, has_more={}, next_offset={}",
            p1.items.len(),
            p1.has_more,
            p1.next_offset
        );
        let p2 = fetch_feed(p1.next_offset, None, &env)
            .await
            .expect("第二页（body offset 翻页）");
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

    /// 封面 URL 形态检查。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_covers() {
        let env = anon_env();
        let p = fetch_feed(0, None, &env).await.expect("feed");
        for item in p.items.iter().take(6) {
            println!("[cover] {} | {}", item.series_id, item.cover);
        }
    }

    /// 信息流条目的**公开计数**字段盘点（hgplayer 未登录也显示
    /// 点赞/评论/收藏数——数据源应是 feed 条目本身，与登录态无关）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_counts() {
        let env = anon_env();
        // 直接取原始响应看字段
        let biz_query = vec![("offset".to_string(), "0".to_string())];
        let body = serde_json::to_vec(&serde_json::json!({ "biz_param": {} })).unwrap();
        let bytes = super::super::client::api_call_full(
            API_ORIGIN,
            LANDPAGE_PATH,
            Some(body),
            &biz_query,
            &env,
        )
        .await
        .expect("feed 原始响应");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let first = &v["data"]["video_data"][0];
        let keys: Vec<String> = first
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let count_keys: Vec<&String> =
            keys.iter().filter(|k| k.contains("count") || k.contains("cnt")).collect();
        println!("[feed-counts] 条目全部计数类键: {count_keys:?}");
        for k in count_keys {
            println!("  {k} = {}", first[k]);
        }
        // 多打几条看 comment_count/digged_count 的典型值
        if let Some(items) = v["data"]["video_data"].as_array() {
            for it in items.iter().take(5) {
                println!(
                    "[feed-counts] {} digged={:?} comment={:?} followed={:?}",
                    it["series_id"].as_str().unwrap_or("?"),
                    it.get("digged_count"),
                    it.get("comment_count"),
                    it.get("followed_cnt"),
                );
            }
        }
    }

    /// 弹幕/评论列表参数探测：code==0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_comment_list() {
        let env = anon_env();
        // 先从信息流拿一个真实 vid 做锚点
        let feed = fetch_feed(0, None, &env).await.expect("信息流");
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

    /// 弹幕列表直连验证（抓包形状：vid 在路径里、参数在顶层、
    /// x-reading-request 头 = ticket-random）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_danmaku_verified() {
        let env = anon_env();
        // 抓包锚点：hgplayer 续播的那集
        let group_id = "7690197301075119166";
        let book_id = "7690150906532219966";
        let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
        let body = serde_json::json!({
            "aid": 8662,
            "business_param": {
                "book_id": book_id,
                "need_danmaku_guide_type": [],
                "playlet_item_duration": 60000,
                "start_offset_time": 0,
            },
            "comment_source": 601,
            "comment_type": 20,
            "compliance_status": 0,
            "count": 90,
            "cursor": "",
            "group_id": group_id,
            "group_type": 30,
            "server_channel": 1000,
            "sort": 1,
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        // x-reading-request: {ticket_ms}-{random}
        let ticket = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let rnd: u32 = rand::random();
        let extra = [(
            "x-reading-request".to_string(),
            format!("{ticket}-{rnd}"),
        )];
        let origin = "https://api5-normal-lq.fqnovel.com";
        match crate::domain::api::client::api_call_full_with_headers(
            origin, &path, Some(bytes), &[], &extra, &env,
        )
        .await
        {
            Ok(b) => {
                let v: Value = serde_json::from_slice(&b).unwrap_or(Value::Null);
                let s = serde_json::to_string(&v).unwrap_or_default();
                println!("[danmaku-ok] {}", s.chars().take(600).collect::<String>());
            }
            Err(e) => println!("[danmaku-ok] ERR {e}"),
        }
    }

    /// bookmall 重放（抓包参数）：GET + 全业务 query。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_bookmall_verified() {
        let env = anon_env();
        // 干净重放：设备字段全进 DeviceProfile（签名器只拼这一套），业务参数只留
        // 非设备项，UA/版本与设备字段成套（hgplayer 形态）。
        let pairs = vec![
                ("ac", r#"wifi"#),
                ("aid", r#"8662"#),
                ("app_name", r#"novelread"#),
                ("cdid", r#"e9ca8ec4-bcbf-46e2-8e4c-281855bccaae"#),
                ("channel", r#"xiaomi_8662_64"#),
                ("compliance_status", r#"0"#),
                ("device_brand", r#"xiaomi"#),
                ("device_id", r#"2169800441471882"#),
                ("device_platform", r#"android"#),
                ("device_type", r#"23127PN0CC"#),
                ("dpi", r#"460"#),
                ("dragon_device_type", r#"phone"#),
                ("host_abi", r#"arm64-v8a"#),
                ("iid", r#"2169800441475978"#),
                ("is_android_pad_screen", r#"0"#),
                ("language", r#"zh"#),
                ("manifest_version_code", r#"73932"#),
                ("need_personal_recommend", r#"1"#),
                ("os", r#"android"#),
                ("os_api", r#"34"#),
                ("os_version", r#"14"#),
                ("player_so_load", r#"1"#),
                ("pv_player", r#"73932"#),
                ("resolution", r#"1200*2670"#),
                ("ssmix", r#"a"#),
                ("update_version_code", r#"73932"#),
                ("version_code", r#"73932"#),
                ("version_name", r#"7.3.9.32"#),
        ];
        let mut v = serde_json::to_value(crate::signer::video_device()).unwrap();
        v["fields"] = serde_json::Value::Array(
            pairs
                .into_iter()
                .map(|(k, val): (&str, &str)| serde_json::json!([k, val]))
                .collect(),
        );
        v["user_agent"] = serde_json::Value::String(
            "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)".into(),
        );
        let _hg_env = crate::domain::api::client::ApiEnv {
            proxy: env.proxy.clone(),
            device: serde_json::from_value(v).unwrap(),
            cookie: Some(
                "store-region=cn-gd; store-region-src=did; install_id=2169800441475978; ttreq=1$f814f969f9b9fe3c43ab008c4e981e84d23a6b4d".into(),
            ),
            x_tt_token: None,
        };
        // 对照：我们自己的静态档案，仅版本号升到 73932（UA 保留我们的机型）
        let mut own = crate::signer::video_device();
        for key in ["version_code", "manifest_version_code", "update_version_code", "pv_player"] {
            own.set(key, "73932");
        }
        own.set("version_name", "7.3.9.32");
        let mut ov = serde_json::to_value(&own).unwrap();
        ov["user_agent"] = serde_json::Value::String(
            "com.phoenix.read/73932 (Linux; U; Android 16; zh_CN; 25053RT47C; Build/BP2A.250605.031.A3; Cronet/TTNetVersion:04657795 2026-01-23 QuicVersion:c67e9834 2025-09-08)".into(),
        );
        let own_dev: crate::signer::device::DeviceProfile = serde_json::from_value(ov).unwrap();
        let env = crate::domain::api::client::ApiEnv {
            proxy: env.proxy.clone(),
            device: own_dev,
            cookie: None,
            x_tt_token: None,
        };
        let q: Vec<(String, String)> = vec![
            ("auth_aweme", r#"true"#),
            ("auth_backward", r#"true"#),
            ("bottom_tab_type", r#"7"#),
            ("client_req_type", r#"60"#),
            ("device_level", r#"3"#),
            ("disable_digg_stat", r#"false"#),
            ("has_video_cache", r#"false"#),
            ("is_horizontal_screen", r#"false"#),
            ("landing_bottom_tab_type", r#"7"#),
            ("last_tab_index", r#"0"#),
            ("last_tab_type", r#"0"#),
            ("offset", r#"0"#),
            ("req_rank_category_id", r#"0"#),
            ("screen_width_px", r#"1078"#),
            ("session_id", r#""#),
            ("stream_count", r#"[{"scene":"1","StreamCount":1,"StreamType":"1"}]"#),
            ("tab_index", r#"0"#),
            ("tab_type", r#"16"#),
            ("video_type_preferences_str", r#"[]"#),
        ]
        .into_iter()
        .map(|(k, v): (&str, &str)| (k.to_string(), v.to_string()))
        .collect();

        // 对照实验：hgplayer 抓包里的设备 cookie（install_id/ttreq 是
        // 设备注册回执）。带与不带各打一次，隔离 ILLEGAL_ACCESS 的成因。
        let mut with_cookie = env.clone();
        with_cookie.cookie = Some("passport_csrf_token=; passport_csrf_token_default=; store-region=cn-gd; store-region-src=did; install_id=2169800441475978; ttreq=1$f814f969f9b9fe3c43ab008c4e981e84d23a6b4d".to_string());
        let no_cookie = crate::domain::api::client::api_call_full(
            crate::domain::api::danmaku::LQ_API_ORIGIN,
            "/reading/bookapi/bookmall/tab/v",
            None,
            &q,
            &env,
        )
        .await;
        match &no_cookie {
            Ok(b) => {
                let v: Value = serde_json::from_slice(b).unwrap_or(Value::Null);
                println!(
                    "[bookmall] no-cookie: code={:?} msg={:?}",
                    v.get("code"), v.get("message")
                );
            }
            Err(e) => println!("[bookmall] no-cookie ERR {e}"),
        }
        match crate::domain::api::client::api_call_full(
            crate::domain::api::danmaku::LQ_API_ORIGIN,
            "/reading/bookapi/bookmall/tab/v",
            None,
            &q,
            &with_cookie,
        )
        .await
        {
            Ok(b) => {
                std::fs::write("C:/Users/liu13/AppData/Local/Temp/hg_capture/bookmall.json", &b).ok();
                let v: Value = serde_json::from_slice(&b).unwrap_or(Value::Null);
                let tabs = v.pointer("/data/tab_item").and_then(Value::as_array).cloned().unwrap_or_default();
                println!(
                    "[bookmall] code={:?} tabs={:?}",
                    v.get("code"),
                    tabs.iter().map(|t| format!(
                        "{}(type={},cells={})",
                        t.get("title").and_then(Value::as_str).unwrap_or("?"),
                        t.get("tab_type").and_then(Value::as_i64).unwrap_or(-1),
                        t.get("cell_data").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)
                    )).collect::<Vec<_>>()
                );
                for t in &tabs {
                    if let Some(cells) = t.get("cell_data").and_then(Value::as_array) {
                        for c in cells {
                            if let Some(inner) = c.get("cell_data").and_then(Value::as_array) {
                                for x in inner {
                                    if let Some(vd) = x.get("video_data").and_then(Value::as_array).and_then(|a| a.first()) {
                                        println!(
                                            "[bookmall] {} keys: {:?}",
                                            t.get("title").and_then(Value::as_str).unwrap_or("?"),
                                            vd.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()
                                        );
                                        println!("[bookmall] sample: {}", serde_json::to_string(vd).unwrap_or_default().chars().take(700).collect::<String>());
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => println!("[bookmall] ERR {e}"),
        }
    }

    /// 预约日历：subscribe/list 带 need_calendar_schema=true（抓包形状）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_subscribe_calendar() {
        let env = anon_env();
        let q: Vec<(String, String)> = vec![
            ("is_online", "true"),
            ("limit", "20"),
            ("need_calendar_schema", "true"),
            ("offset", "0"),
            ("subscribe_offset", "0"),
            ("subscribe_order_type", "0"),
            ("swipe_type", "0"),
            ("tab_type", "13"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        match crate::domain::api::client::api_call_full(
            crate::domain::api::danmaku::LQ_API_ORIGIN,
            "/reading/user/subscribe/list/v1/",
            None,
            &q,
            &env,
        )
        .await
        {
            Ok(b) => println!(
                "[subscribe-cal] {}",
                String::from_utf8_lossy(&b).chars().take(1200).collect::<String>()
            ),
            Err(e) => println!("[subscribe-cal] ERR {e}"),
        }
    }

    /// 预约日历（uncover_subscribe）：GET on LQ + 日期范围（CalendarPage(ctx, start, end) 双 string 签名）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_uncover_subscribe() {
        let env = anon_env();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let week = chrono::Local::now()
            .checked_add_days(chrono::Days::new(7))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| today.clone());
        let candidates: Vec<(&str, Vec<( &str, &str)>)> = vec![
            ("range", vec![("start_date", &today), ("end_date", &week)]),
            ("date", vec![("date", &today)]),
            (
                "full",
                vec![
                    ("is_online", "true"),
                    ("limit", "20"),
                    ("need_calendar_schema", "true"),
                    ("offset", "0"),
                    ("tab_type", "13"),
                ],
            ),
        ];
        for (tag, base) in candidates {
            let q: Vec<(String, String)> = base
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            match crate::domain::api::client::api_call_full(
                crate::domain::api::danmaku::LQ_API_ORIGIN,
                "/reading/bookapi/search/uncover_subscribe/v1/",
                None,
                &q,
                &env,
            )
            .await
            {
                Ok(b) => println!(
                    "[uncover/{tag}] {}",
                    String::from_utf8_lossy(&b).chars().take(700).collect::<String>()
                ),
                Err(e) => println!("[uncover/{tag}] ERR {e}"),
            }
        }
    }

}

#[cfg(test)]
mod probe_extra {
    use super::*;

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_content_type_values() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        let body = serde_json::to_vec(&serde_json::json!({ "biz_param": {} })).unwrap();
        let bytes = super::super::client::api_call_full(
            API_ORIGIN,
            LANDPAGE_PATH,
            Some(body),
            &[("offset".to_string(), "0".to_string())],
            &env,
        )
        .await
        .expect("feed");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let items = v.pointer("/data/video_data").and_then(Value::as_array).cloned().unwrap_or_default();
        for it in items.iter() {
            println!(
                "[ctype] content_type={:?} tags={:?} title={:?}",
                it.get("content_type"),
                it.get("category_schema").and_then(Value::as_str).map(|s| &s[..s.len().min(90)]),
                it.get("title").and_then(Value::as_str)
            );
        }
    }
}

#[cfg(test)]
mod probe_genre {
    use super::*;

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_genre_filter() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        for (name, genre) in [
            ("漫剧", Some(GENRE_COMIC)),
            ("真人剧", Some(GENRE_HUMAN)),
            ("全部", None),
        ] {
            let p1 = fetch_feed(0, genre, &env).await.expect("第一页");
            println!(
                "[genre] {name}: {} 条 hasMore={} next={}",
                p1.items.len(),
                p1.has_more,
                p1.next_offset
            );
            for it in p1.items.iter().take(8) {
                println!("[genre]   ct={:<4} {}", it.content_type, it.title);
            }
            if p1.has_more {
                let p2 = fetch_feed(p1.next_offset, genre, &env).await.expect("第二页");
                let dup = p2
                    .items
                    .iter()
                    .filter(|b| p1.items.iter().any(|a| a.series_id == b.series_id))
                    .count();
                println!("[genre] {name} 第二页: {} 条 重复={}", p2.items.len(), dup);
            }
        }
    }
}
