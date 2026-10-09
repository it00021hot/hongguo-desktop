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

use super::client::{ApiEnv, api_call_full};
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
    /// 季角标（sub_title_list data_type=0，「第1季」形态；hgplayer titleTag 同源）
    #[serde(default)]
    pub season_tag: String,
    /// 热度文本（sub_title_list data_type=27，「1705万」形态，配火焰图标展示）
    #[serde(default)]
    pub heat_text: String,
    /// 官方运营角标（tag_info.text：「新剧/爆剧/红果首发」等；2026-10-07 抓包实证）
    #[serde(default)]
    pub badge: String,
    /// 内容类型：1=真人剧，1004=漫剧（推荐流「按类型刷」的过滤键）
    #[serde(default)]
    pub content_type: i64,
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

// 首页推荐流已迁移到书城 cell 换一换（recommend 模块 fetch_recommend_feed，
// 2026-10-08 对齐 hgplayer RecommendTab）；landpage 只服务找剧筛选浏览。

// ---------------------------------------------------------------- 找剧（筛选浏览）

/// 找剧筛选面板的一行（一个维度）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorItem {
    /// 选项 id（select_items 的取值，如 `short_play`/`cate_262`/`days_7`）
    pub id: String,
    /// 展示名（如 `真人剧`/`脑洞`/`7天内上新`）
    pub name: String,
}

/// 找剧筛选面板的一行。`row_type` 即 select_items 的键
/// （genre/category_dim_theme/category_dim_role/category_dim_epoch/
/// sort/gender/online_time/duration）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorRow {
    pub row_type: String,
    /// 服务端行名（`全部体裁`…，行头「全部」态即空选）
    pub row_name: String,
    pub items: Vec<SelectorItem>,
}

/// 找剧的筛选条件（每维至多一个选中值，空串/None = 全部）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowseFilters {
    pub genre: String,
    pub theme: String,
    pub role: String,
    pub epoch: String,
    /// 推荐：`online_time`(最新上架)/`hot_score`(最高热度)/`hot_collect`(最高收藏)
    pub sort: String,
    /// 受众：`1`=男频 `0`=女频
    pub gender: String,
    /// 上新时间：`days_7`/`days_14`/`days_30`/`days_90`
    pub online_time: String,
    /// 长度：`duration_0_60`/`duration_60_120`/`duration_120_plus`
    pub duration: String,
}

impl BrowseFilters {
    /// select_items 请求形态：每维一个单元素数组（空选给空数组）。
    fn to_select_items(&self) -> Value {
        let one = |v: &str| {
            if v.is_empty() {
                Value::Array(vec![])
            } else {
                serde_json::json!([v])
            }
        };
        serde_json::json!({
            "category_dim_epoch": one(&self.epoch),
            "category_dim_role": one(&self.role),
            "category_dim_theme": one(&self.theme),
            "duration": one(&self.duration),
            "gender": one(&self.gender),
            "genre": one(&self.genre),
            "online_time": one(&self.online_time),
            "sort": one(&self.sort),
        })
    }
}

/// 拉找剧筛选面板（八行维度选项，选项表随服务端运营变化，不落死）。
///
/// 抓包形态（2026-10-07 hgplayer 1.1.5）：body 只有三个键，
/// 响应 `data.selector_rows[]`。
pub async fn fetch_browse_panel(env: &ApiEnv) -> AppResult<Vec<SelectorRow>> {
    let body = serde_json::to_vec(&serde_json::json!({
        "need_selector_panel": false,
        "req_scene": "default",
        "req_type": "only_panel",
    }))
    .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    let data = value
        .get("data")
        .ok_or_else(|| AppError::Media("面板响应缺少 data".into()))?;
    parse_browse_panel(Some(data))
}

/// 解析面板 data 节点为 selector 行列表。
fn parse_browse_panel(data: Option<&Value>) -> AppResult<Vec<SelectorRow>> {
    let data = data.ok_or_else(|| AppError::Media("面板响应缺少 data".into()))?;
    let mut rows = Vec::new();
    for raw in data
        .get("selector_rows")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let row_type = str_field(raw, "type");
        if row_type.is_empty() {
            continue;
        }
        let items = raw
            .get("items")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| {
                        let id = it.get("selector_item_id").and_then(Value::as_str)?;
                        Some(SelectorItem {
                            id: id.to_string(),
                            name: it
                                .get("show_name")
                                .and_then(Value::as_str)
                                .unwrap_or(id)
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.push(SelectorRow {
            row_type,
            row_name: str_field(raw, "row_name"),
            items,
        });
    }
    Ok(rows)
}

/// 拉一页找剧结果（与推荐流同端点，多维 select_items 服务端过滤）。
///
/// `session_id` 首页传空串，翻页传上一页响应里的值（服务端按它记住筛选上下文）。
/// 每页 20 条（用户指定；响应 next_offset 游标会跟着走，翻页无算术依赖）。
pub async fn fetch_browse(
    filters: &BrowseFilters,
    offset: i64,
    session_id: &str,
    env: &ApiEnv,
) -> AppResult<FeedPage> {
    let body = serde_json::to_vec(&serde_json::json!({
        "client_req_type": 3,
        "filter_ids": "",
        "limit": 20,
        "need_selector_panel": false,
        "offset": offset,
        "req_scene": "default",
        "req_type": "only_content",
        "select_items": filters.to_select_items(),
        "session_id": session_id,
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
    if let Some(code) = value.get("code").and_then(Value::as_i64)
        && code != 0
    {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(AppError::Media(format!("接口返回 {code}: {msg}")));
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
        // sub_title_list：data_type 0=季文本 / 3=分类 / 27=热度（2026-10-07
        // 抓包实证；分类沿用 category_schema 解析，这里只取季与热度）
        let (season_tag, heat_text) = parse_sub_titles(raw.get("sub_title_list"));
        // 官方运营角标：tag_info（同名字段在 plan/v 里是「第N季/同IP」，
        // 在 landpage 信息流里是「新剧/爆剧/红果首发」，enable=false 不显）
        let badge = raw
            .pointer("/tag_info/text")
            .and_then(Value::as_str)
            .filter(|_| raw.pointer("/tag_info/enable").and_then(Value::as_bool) != Some(false))
            .unwrap_or("")
            .to_string();
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
            season_tag,
            heat_text,
            badge,
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

/// sub_title_list → (季文本, 热度文本)。data_type 语义见 2026-10-07 抓包：
/// 0=「第1季」形态、27=热度数值文本（官方配火焰图标）、3=分类（另有
/// category_schema 承载，这里不取）。两条都算展示增强，缺了给空串。
fn parse_sub_titles(list: Option<&Value>) -> (String, String) {
    let mut season = String::new();
    let mut heat = String::new();
    for it in list
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(content) = it.get("content").and_then(Value::as_str) else {
            continue;
        };
        match it.get("data_type").and_then(Value::as_i64) {
            Some(0) if season.is_empty() => season = content.to_string(),
            Some(27) if heat.is_empty() => heat = content.to_string(),
            _ => {}
        }
    }
    (season, heat)
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

    /// 面板解析：行 type/row_name/选项 id+名（2026-10-07 抓包样本的形状）。
    #[test]
    fn parses_browse_panel_rows() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": {
                "selector_rows": [
                    {
                        "type": "genre",
                        "row_name": "全部体裁",
                        "selection_type": 2,
                        "items": [
                            { "selector_item_id": "short_play", "show_name": "真人剧" },
                            { "selector_item_id": "comic_series", "show_name": "漫剧" },
                            { "selector_item_id": "ai_series", "show_name": "AI剧" }
                        ]
                    },
                    {
                        "type": "sort",
                        "row_name": "全部推荐",
                        "items": [
                            { "selector_item_id": "online_time", "show_name": "最新上架" }
                        ]
                    },
                    { "type": "", "row_name": "坏行", "items": [] }
                ]
            }
        });
        let rows = parse_browse_panel(v.get("data")).unwrap();
        assert_eq!(rows.len(), 2, "空 type 的行要跳过");
        assert_eq!(rows[0].row_type, "genre");
        assert_eq!(rows[0].items.len(), 3);
        assert_eq!(rows[0].items[0].id, "short_play");
        assert_eq!(rows[0].items[0].name, "真人剧");
        assert_eq!(rows[1].row_type, "sort");
    }

    /// 筛选条件 → select_items：选中值包单元素数组，空选给空数组。
    #[test]
    fn browse_filters_build_select_items() {
        let f = BrowseFilters {
            genre: "comic_series".into(),
            online_time: "days_7".into(),
            ..Default::default()
        };
        let si = f.to_select_items();
        assert_eq!(si["genre"], serde_json::json!(["comic_series"]));
        assert_eq!(si["online_time"], serde_json::json!(["days_7"]));
        assert_eq!(si["sort"], serde_json::json!([]));
        // duration 维度也要在场（面板有「长度」行，2026-10-07 抓包）
        assert!(si.get("duration").is_some());
        assert_eq!(si["duration"], serde_json::json!([]));
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
        let extra = [("x-reading-request".to_string(), format!("{ticket}-{rnd}"))];
        let origin = "https://api5-normal-lq.fqnovel.com";
        match crate::domain::api::client::api_call_full_with_headers(
            origin,
            &path,
            Some(bytes),
            &[],
            &extra,
            &env,
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
        for key in [
            "version_code",
            "manifest_version_code",
            "update_version_code",
            "pv_player",
        ] {
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
            (
                "stream_count",
                r#"[{"scene":"1","StreamCount":1,"StreamType":"1"}]"#,
            ),
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
                    v.get("code"),
                    v.get("message")
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
                let dir = crate::domain::api::capture_dir();
                std::fs::create_dir_all(&dir).ok();
                std::fs::write(dir.join("bookmall.json"), &b).ok();
                let v: Value = serde_json::from_slice(&b).unwrap_or(Value::Null);
                let tabs = v
                    .pointer("/data/tab_item")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                println!(
                    "[bookmall] code={:?} tabs={:?}",
                    v.get("code"),
                    tabs.iter()
                        .map(|t| format!(
                            "{}(type={},cells={})",
                            t.get("title").and_then(Value::as_str).unwrap_or("?"),
                            t.get("tab_type").and_then(Value::as_i64).unwrap_or(-1),
                            t.get("cell_data")
                                .and_then(Value::as_array)
                                .map(|a| a.len())
                                .unwrap_or(0)
                        ))
                        .collect::<Vec<_>>()
                );
                for t in &tabs {
                    if let Some(cells) = t.get("cell_data").and_then(Value::as_array) {
                        for c in cells {
                            if let Some(inner) = c.get("cell_data").and_then(Value::as_array) {
                                for x in inner {
                                    if let Some(vd) = x
                                        .get("video_data")
                                        .and_then(Value::as_array)
                                        .and_then(|a| a.first())
                                    {
                                        println!(
                                            "[bookmall] {} keys: {:?}",
                                            t.get("title").and_then(Value::as_str).unwrap_or("?"),
                                            vd.as_object()
                                                .map(|o| o.keys().cloned().collect::<Vec<_>>())
                                                .unwrap_or_default()
                                        );
                                        println!(
                                            "[bookmall] sample: {}",
                                            serde_json::to_string(vd)
                                                .unwrap_or_default()
                                                .chars()
                                                .take(700)
                                                .collect::<String>()
                                        );
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
                String::from_utf8_lossy(&b)
                    .chars()
                    .take(1200)
                    .collect::<String>()
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
        let candidates: Vec<(&str, Vec<(&str, &str)>)> = vec![
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
                    String::from_utf8_lossy(&b)
                        .chars()
                        .take(700)
                        .collect::<String>()
                ),
                Err(e) => println!("[uncover/{tag}] ERR {e}"),
            }
        }
    }
}

#[cfg(test)]
mod probe_browse {
    use super::*;
    /// 找剧面板 + 首页结果真连探测（2026-10-07 端点实装后转正的验证）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_browse_panel_and_page() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let rows = fetch_browse_panel(&env).await.expect("面板");
        for r in &rows {
            println!(
                "[browse] [{}] {} ({} 项)",
                r.row_type,
                r.row_name,
                r.items.len()
            );
        }
        let filters = BrowseFilters {
            genre: "comic_series".into(),
            ..Default::default()
        };
        let page = fetch_browse(&filters, 0, "", &env).await.expect("找剧首页");
        println!(
            "[browse] 漫剧筛选首页 {} 条, has_more={}, 首条: {}",
            page.items.len(),
            page.has_more,
            page.items.first().map(|i| i.title.as_str()).unwrap_or("-")
        );
        assert!(!rows.is_empty());
        assert!(!page.items.is_empty());
    }
}

#[cfg(test)]
mod probe_panel_cookie {
    use super::*;
    /// 带登录 cookie 的面板请求——对比匿名探测（8 行）验证服务端是否
    /// 按登录态下发不同面板（app 实测只有 7 行，缺 duration）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_browse_panel_with_cookies() {
        let mut env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let cookies = std::env::var("HG_LOGIN_COOKIES").unwrap_or_default();
        if !cookies.is_empty() {
            env.cookie = Some(cookies);
        }
        let rows = fetch_browse_panel(&env).await.expect("面板");
        println!(
            "[panel-cookie] {} 行: {:?}",
            rows.len(),
            rows.iter().map(|r| r.row_type.as_str()).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod probe_duration {
    use super::*;
    /// duration 维度筛选是否被服务端接受（面板可能按设备不下发该行，
    /// 但 select_items 带它要有意义才算可用）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_browse_duration_filter() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let filters = BrowseFilters {
            duration: "duration_0_60".into(),
            ..Default::default()
        };
        let page = fetch_browse(&filters, 0, "", &env)
            .await
            .expect("duration 筛选");
        println!(
            "[duration] 0-60分钟筛选: {} 条, has_more={}, 首条: {}",
            page.items.len(),
            page.has_more,
            page.items.first().map(|i| i.title.as_str()).unwrap_or("-")
        );
        let all = fetch_browse(&BrowseFilters::default(), 0, "", &env)
            .await
            .expect("无筛选对照");
        println!("[duration] 无筛选对照: {} 条", all.items.len());
    }
}
