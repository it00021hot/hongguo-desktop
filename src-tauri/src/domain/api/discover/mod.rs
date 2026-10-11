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

mod model;
mod parse;

use serde_json::Value;

use super::client::{ApiEnv, api_call_full};
use crate::error::{AppError, AppResult};
use crate::signer::API_ORIGIN;

pub use model::{BrowseFilters, FeedItem, FeedPage, SelectorRow};
// 保持既有公共路径 `discover::SelectorItem` 不变（crate 内暂无直接引用者）
#[expect(
    unused_imports,
    reason = "SelectorItem 只被 SelectorRow 聚合引用，re-export 为兼容旧路径保留"
)]
pub use model::SelectorItem;
// JSON/字段助手已收敛进 utils::json（P3-C9）；这里再导出，保持
// `super::discover::X` 的既有跨域引用路径不变（rank/search/… 共 6 域）。
pub(crate) use crate::utils::json::{check_code, int_field, num_field, parse_tags, str_field};

use parse::{parse_browse_panel, parse_feed};

/// 推荐信息流落地页。
pub const LANDPAGE_PATH: &str = "/reading/distribution/category/landpage/v1/";

// 首页推荐流已迁移到书城 cell 换一换（recommend 模块 fetch_recommend_feed，
// 2026-10-08 对齐 hgplayer RecommendTab）；landpage 只服务找剧筛选浏览。

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

/// 拉一页找剧结果（与推荐流同端点，多维 select_items 服务端过滤）。
///
/// `session_id` 首页传空串，翻页传上一页响应里的值（服务端按它记住筛选上下文）。
/// 每页 20 条（用户指定；响应 next_offset 游标会跟着走，翻页无算术依赖）。
///
/// 2026-10-11 抓 hgplayer 1.1.8 实流对齐：`creation_status` 仅在选中时在参，
/// 全部选省略整个键（他家不发空串）。
pub async fn fetch_browse(
    filters: &BrowseFilters,
    offset: i64,
    session_id: &str,
    env: &ApiEnv,
) -> AppResult<FeedPage> {
    let mut body = serde_json::json!({
        "client_req_type": 3,
        "filter_ids": "",
        "limit": 20,
        "need_selector_panel": false,
        "offset": offset,
        "req_scene": "default",
        "req_type": "only_content",
        "select_items": filters.to_select_items(),
        "session_id": session_id,
    });
    // 完结状态筛选（hgplayer 1.1.8 同款）：客户端合成的「完结状态」行选中值
    // 走 body 顶层；全部选时不发该键（抓包实锤他家省略）
    if !filters.creation_status.is_empty() {
        body["creation_status"] = serde_json::Value::String(filters.creation_status.clone());
    }
    let body = serde_json::to_vec(&body)
    .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    parse_feed(value.get("data"))
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
