//! 首页推荐流接口（书城换一换；详见 docs/hongguo-api-endpoints.md）。

mod cache;
mod model;
mod parse;
mod session;

use serde_json::Value;

use super::client::{ApiEnv, SINFONLINEC_API_ORIGIN, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use super::rank::RANK_CELL_PATH;
use hongguo_common::{AppError, AppResult};

pub use cache::clear_feed_tabs_cache;
pub use model::{FeedTabMeta, RecommendTabConfig};

use cache::{
    TAB_CONFIG_TTL, cache_feed_tabs_insert, cache_tab_config_insert, cached_feed_tabs,
    cached_tab_config, flex_str, known_tab_config, parse_feed_tabs,
};
use parse::{parse_recommend_feed, recommend_feed_entry};
use session::{
    FEED_IMAGE_SHRINK, NPS_INSERT_RANK_JSON, RECOMMEND_EXTRA, rec_filter_info_list,
    recent_impressions, session_runtime_pairs, session_uuid,
};

pub async fn fetch_recommend_feed(
    tab: &str,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    env: &ApiEnv,
) -> AppResult<super::discover::FeedPage> {
    let page = if session_id.is_empty() {
        let (cfg, mut page) = fetch_tab_first_page(tab, env).await?;
        // 首批偶发空批：带着首页下发的会话直接走 cell/change 补拉
        if page.items.is_empty()
            && !page.session_id.is_empty()
            && let Ok(fill) = recommend_cell_change(tab, &cfg, &page.session_id, 0, &[], env).await
        {
            page.items = fill.items;
            page.has_more = fill.has_more;
            page.next_offset = fill.next_offset;
        }
        cache_tab_config_insert(tab, cfg);
        page
    } else {
        let cfg = resolve_tab_config(tab, env).await?;
        recommend_cell_change(tab, &cfg, session_id, offset, filter_ids, env).await?
    };
    // 入口 vid 的条目类型进注册表：批量取流（预取/真播）的分组 key 来源
    for item in &page.items {
        super::play_url::register_content_type(&item.vid, item.content_type);
    }
    Ok(page)
}

/// 推荐 tab 在顶栏里的 tab_type：顶栏列表请求的源 tab（其响应 `tab_item[]`
/// 即完整顶栏），也是种子语义的归属 tab（「你的内容」只进推荐 tab——壳侧口径）。
const RECOMMEND_TAB_TYPE: &str = "16";

/// 顶栏 tab 列表（服务端下发，一次 cr=4 请求的 `tab_item[]` 即完整顶栏）。
/// 单 tab 结果视为身份未就绪：不缓存、返回空表由调用方回落；复验失败沿用旧值。
pub async fn fetch_feed_tabs(env: &ApiEnv) -> AppResult<Vec<FeedTabMeta>> {
    if let Some(cached) = cached_feed_tabs() {
        if cached.at.elapsed() < TAB_CONFIG_TTL {
            return Ok(cached.tabs);
        }
        match request_bookstore_tab(RECOMMEND_TAB_TYPE, env).await {
            Ok(value) => {
                let tabs = parse_feed_tabs(&value)?;
                log::info!("[Recommend] 顶栏列表复验更新（{} 个 tab）", tabs.len());
                cache_feed_tabs_insert(tabs.clone());
                return Ok(tabs);
            }
            Err(e) => {
                log::warn!(
                    "[Recommend] 顶栏列表复验失败（{e}），沿用 {:?} 前的旧值",
                    cached.at.elapsed()
                );
                return Ok(cached.tabs);
            }
        }
    }
    let value = request_bookstore_tab(RECOMMEND_TAB_TYPE, env).await?;
    let tabs = parse_feed_tabs(&value)?;
    if tabs.len() <= 1 {
        log::info!(
            "[Recommend] 顶栏塌缩为 {} 个 tab（身份未就绪），不缓存，由调用方回落",
            tabs.len()
        );
        return Ok(Vec::new());
    }
    cache_feed_tabs_insert(tabs.clone());
    Ok(tabs)
}

async fn resolve_tab_config(tab: &str, env: &ApiEnv) -> AppResult<RecommendTabConfig> {
    if let Some(cached) = cached_tab_config(tab) {
        if cached.at.elapsed() < TAB_CONFIG_TTL {
            return Ok(cached.cfg);
        }
        // 过期：重取一次做复验；失败不抛——旧值总比回落已知值新
        match fetch_tab_first_page(tab, env).await {
            Ok((cfg, _)) => {
                log::info!("[Recommend] tab={tab} 配置过期已复验更新");
                cache_tab_config_insert(tab, cfg.clone());
                return Ok(cfg);
            }
            Err(e) => {
                log::warn!(
                    "[Recommend] tab={tab} 配置复验失败（{e}），沿用 {TAB_CONFIG_TTL:?} 前的旧值"
                );
                return Ok(cached.cfg);
            }
        }
    }
    let fetched = match fetch_tab_first_page(tab, env).await {
        Ok((cfg, _)) => Some(cfg),
        Err(e) => {
            log::warn!("[Recommend] tab={tab} 配置现场取失败（{e}），回落抓包已知值");
            None
        }
    };
    let value = fetched.or_else(|| known_tab_config(tab)).ok_or_else(|| {
        AppError::Media(format!("未知推荐 tab 且配置不可用: {tab}（应为 16/36/39）"))
    })?;
    cache_tab_config_insert(tab, value.clone());
    Ok(value)
}

/// `GET bookmall/tab/v?client_req_type=4`（lq 域）——顶栏塌缩表专用；
/// cr=1028（书城形态）返回的是书城九 tab，不是本仓首页要的表。
async fn request_bookstore_tab(tab: &str, env: &ApiEnv) -> AppResult<Value> {
    let q: Vec<(String, String)> = [
        ("auth_aweme", "true"),
        ("auth_backward", "true"),
        ("bottom_tab_type", "7"),
        ("client_req_type", "4"),
        ("device_level", "3"),
        ("disable_digg_stat", "false"),
        ("has_video_cache", "false"),
        ("is_horizontal_screen", "false"),
        ("landing_bottom_tab_type", "7"),
        ("last_tab_index", "0"),
        ("last_tab_type", "0"),
        ("offset", "0"),
        ("req_rank_category_id", "0"),
        ("screen_width_px", "1078"),
        (
            "stream_count",
            r#"[{"scene":"1","StreamCount":1,"StreamType":"1"}]"#,
        ),
        ("tab_index", "0"),
        ("tab_type", tab),
        ("video_type_preferences_str", "[]"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_reading(
        LQ_API_ORIGIN,
        "/reading/bookapi/bookmall/tab/v",
        None,
        &q,
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析推荐 tab 首页失败: {e}")))?;
    check_code(&value)?;
    Ok(value)
}

async fn request_feed_tab_first_page(tab: &str, env: &ApiEnv) -> AppResult<Value> {
    let uuid = session_uuid();
    let q: Vec<(String, String)> = [
        ("filter_ids", ""),
        ("last_search_query_from_rec", "false"),
        ("rec_filter_info_list", ""),
        ("bottom_tab_type_list", "7,0,1,2,3,4"),
        ("last_tab_index", "0"),
        ("bookmall_video_last_active_enter_time", "0"),
        ("is_video_feed_tab_first_request_cold_start", "true"),
        ("is_horizontal_screen", "false"),
        ("screen_width_px", "1078"),
        ("tab_type", tab),
        ("first_use_category_select", "false"),
        ("image_shrink_datas_str", FEED_IMAGE_SHRINK),
        ("last_session_video_tab_type", tab),
        ("disable_digg_stat", "false"),
        ("ecom_refresh_type", "0"),
        ("pad_column_cover", "0"),
        ("last_cold_start_time", "0"),
        ("last_tab_type", tab),
        ("req_rank_category_id", "0"),
        ("ranklist_in_1col_no_consume_show_times", "0"),
        ("video_tab_cold_start", "0"),
        ("offset", "0"),
        ("video_type_preferences_str", "[]"),
        ("auth_aweme", "false"),
        ("cold_start_is_double_gd", "false"),
        ("item_id", "0"),
        ("page_entry_time", "0"),
        ("cold_start_session", "0"),
        ("tab_index", "0"),
        ("bookmall_playlet_last_active_enter_time", "0"),
        ("book_id", "0"),
        ("ecom_impression_start_time", "0"),
        ("enable_search_box_collapse", "false"),
        ("ranklist_in_1col_show_times", "0"),
        ("landing_bottom_tab_type", "0"),
        ("bottom_tab_type", "7"),
        ("top_tab_extra", ""),
        ("migration_top_tab_enable", "false"),
        ("auth_backward", "false"),
        ("video_bottom_tab_bubble_display_count", "0"),
        ("has_video_cache", "false"),
        ("pad_column_detail", "0"),
        ("after_genre_preference_popup", "0"),
        ("client_req_type", "1028"),
        ("req_rank_algo", "0"),
        ("unlimited_short_series_next_offset", "0"),
        ("current_volume", "0"),
        ("rom_version", "V417IR release-keys"),
        ("need_personal_recommend", "1"),
        ("recommend_extra", RECOMMEND_EXTRA),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain(
        session_runtime_pairs(uuid)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v)),
    )
    .collect();
    let bytes = api_call_reading(
        SINFONLINEC_API_ORIGIN,
        "/reading/bookapi/bookmall/tab/v",
        None,
        &q,
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析推荐 tab 首页失败: {e}")))?;
    check_code(&value)?;
    Ok(value)
}

/// tab 首页请求 →（配置, 首页）。注意 tab_item 里的
/// id 字段是 JSON 数字，容忍字符串形态。
async fn fetch_tab_first_page(
    tab: &str,
    env: &ApiEnv,
) -> AppResult<(RecommendTabConfig, super::discover::FeedPage)> {
    let value = request_feed_tab_first_page(tab, env).await?;
    let tabs = value
        .pointer("/data/tab_item")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("推荐 tab 响应缺 tab_item".into()))?;
    let item = tabs
        .iter()
        .find(|t| flex_str(t.get("tab_type")).as_deref() == Some(tab))
        .ok_or_else(|| AppError::Media(format!("推荐 tab 响应无 tab={tab}")))?;
    // 多个 cell 里按内容取推荐流（嵌套 video_data 非空的那个），不能按位置
    // 取——[0] 可能是空的金刚位运营卡。
    let mut cell_id = String::new();
    let mut items = Vec::new();
    let cells = item
        .get("cell_data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for cell in cells {
        let mut cell_items = Vec::new();
        for inner in cell
            .get("cell_data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            for raw in inner
                .get("video_data")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                if let Some(it) = recommend_feed_entry(raw) {
                    cell_items.push(it);
                }
            }
        }
        if cell_items.is_empty() {
            continue;
        }
        if cell_id.is_empty() {
            cell_id = flex_str(cell.get("cell_id_str"))
                .or_else(|| flex_str(cell.get("cell_id")))
                .unwrap_or_default();
            items = cell_items;
        }
    }
    let bookstore_id = flex_str(item.get("bookstore_id"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Media(format!("推荐 tab 配置缺 bookstore_id: tab={tab}")))?;
    if cell_id.is_empty() {
        // 无带条目的 cell（空金刚位场景）：退回 [0] 的 id，空首批由调用方补拉兜底
        cell_id = flex_str(item.pointer("/cell_data/0/cell_id_str"))
            .or_else(|| flex_str(item.pointer("/cell_data/0/cell_id")))
            .unwrap_or_default();
    }
    let cfg = RecommendTabConfig {
        cell_id,
        bookstore_id,
    };
    // 流的分页真值由翻页 cell/change 给，这里乐观置 true，翻到空页自然停；
    // 首批无 offset，首次翻页 offset=0，之后按响应 next_offset 递进
    let page = super::discover::FeedPage {
        session_id: flex_str(item.get("session_id")).unwrap_or_default(),
        has_more: true,
        next_offset: 0,
        items,
    };
    Ok((cfg, page))
}

/// 官方 cell/change 的图片缩略规格（base64(JSON)：360 中图 / 屏宽大图 / 99 小图）。
async fn recommend_cell_change(
    tab: &str,
    cfg: &RecommendTabConfig,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    env: &ApiEnv,
) -> AppResult<super::discover::FeedPage> {
    let uuid = session_uuid();
    let impressions = recent_impressions();
    let rec_filter_list =
        serde_json::to_string(&rec_filter_info_list(&impressions)).unwrap_or_else(|_| "[]".into());
    let latest_gids = impressions
        .iter()
        .map(|i| i.rec_group_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let refresh_action_info = format!(
        r#"{{"first_screen_impression_gids":[],"has_active_refresh":false,"latest_impression_gids":"{latest_gids}"}}"#
    );
    let q: Vec<(String, String)> = [
        // 业务零值族（官方恒 0/false，缺失会被当无个性化上下文处理）
        ("author_id", "0"),
        ("book_comment_id", "0"),
        ("category_id", "0"),
        ("cell_id", cfg.cell_id.as_str()),
        ("cell_sub_id", "0"),
        ("change_type", "0"),
        ("client_req_type", "2"),
        ("client_template", "7"),
        ("cold_start_session", "0"),
        ("current_chapter_num", "0"),
        ("disable_digg_stat", "false"),
        ("ecom_category_id", "0"),
        ("ecom_impression_start_time", "0"),
        ("ecom_refresh_type", "0"),
        ("ecom_sort_by", "0"),
        ("genre", "0"),
        ("genre_type", "0"),
        ("idol_tag_id", "0"),
        ("image_shrink_datas_str", FEED_IMAGE_SHRINK),
        ("inner_algo", "0"),
        ("inner_category_id", "0"),
        ("is_horizontal_screen", "false"),
        ("item_id", "0"),
        ("limit", "0"),
        ("nps_insert_rank_json", NPS_INSERT_RANK_JSON),
        ("offset", &offset.to_string()),
        ("page", "0"),
        ("page_entry_time", "0"),
        ("plan_id", cfg.bookstore_id.as_str()),
        ("post_id", "0"),
        ("rank_sub_info_id", "0"),
        ("reader_single_col_from_ip_strategy", "false"),
        ("rec_filter_info_list", &rec_filter_list),
        ("recommend_extra", RECOMMEND_EXTRA),
        ("refresh_action_info", &refresh_action_info),
        ("related_book_id", "0"),
        ("screen_width_px", "1078"),
        ("search_tab_type", "0"),
        ("source_tab_type", "0"),
        ("sub_genre", "0"),
        ("sub_tag_id", "0"),
        ("support_gender_list", "false"),
        ("tab_type", tab),
        ("tag_id", "0"),
        ("total_chapter_num", "0"),
        ("unlimited_selector_change_type", "0"),
        ("video_tab_cold_start", "0"),
        ("video_type_preferences_str", "[]"),
        ("web_page_version_code", "0"),
        ("app_launch_times", "0"),
        ("current_volume", "0"),
        ("gender", "2"),
        ("rom_version", "V417IR release-keys"),
        // 排除与去重（翻页语义核心）
        ("session_id", session_id),
        ("filter_ids", &filter_ids.join(",")),
    ]
    .into_iter()
    // 空会话/空排除表不发对应键（与调用方约定一致：首页会话还没建立）
    .filter(|(k, v)| !((*k == "session_id" || *k == "filter_ids") && v.is_empty()))
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain(
        session_runtime_pairs(uuid)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v)),
    )
    .collect();
    // 翻页与 tab 首页同族同源（SINFONLINEC_API_ORIGIN）：会话是首页响应下发的，
    // 跨域复用同一会话有被拒风险
    let bytes = api_call_reading(SINFONLINEC_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析推荐流失败: {e}")))?;
    check_code(&value)?;
    parse_recommend_feed(value.get("data"))
}

#[cfg(test)]
#[path = "recommend_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use crate::rank::probe::anon_env;

    /// 顶栏列表直连探测：游客应得 45/36/39/16 四个 tab，仅 16 带 bookstore_id。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_feed_tabs() {
        let env = anon_env();
        let tabs = fetch_feed_tabs(&env).await.expect("顶栏列表");
        for t in &tabs {
            println!(
                "[tabs] tab_type={:<4} title={:<6} bookstore_id={:?}",
                t.tab_type, t.title, t.bookstore_id
            );
        }
        assert!(tabs.iter().any(|t| t.tab_type == RECOMMEND_TAB_TYPE));
    }

    /// 首页推荐流两段式全链路探测：各 tab 类型过滤与翻页不重复。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_recommend_tab_bookstore() {
        let env = anon_env();
        for tab in ["16", "36", "39"] {
            let p1 = fetch_recommend_feed(tab, "", 0, &[], &env)
                .await
                .expect("首页（bookmall/tab cr=4）");
            println!(
                "[rec:{tab}] #1: {} 条 sess={} ct集合={:?}",
                p1.items.len(),
                &p1.session_id[..p1.session_id.len().min(20)],
                p1.items.iter().map(|i| i.content_type).collect::<Vec<_>>()
            );
            for it in p1.items.iter().take(4) {
                println!(
                    "[rec:{tab}]   ct={:<4} heat={:?} badge={:?} season={:?} score={} | {}",
                    it.content_type, it.heat_text, it.badge, it.season_tag, it.score, it.title
                );
            }
            assert!(!p1.session_id.is_empty(), "tab={tab} 首页应下发会话");
            let seen: Vec<String> = p1.items.iter().map(|i| i.series_id.clone()).collect();
            let p2 = fetch_recommend_feed(tab, &p1.session_id, p1.next_offset, &seen, &env)
                .await
                .expect("翻页（cell/change + filter_ids）");
            let overlap = p2
                .items
                .iter()
                .filter(|b| seen.contains(&b.series_id))
                .count();
            println!(
                "[rec:{tab}] #2: {} 条 more={} 重叠 {overlap} ct集合={:?}",
                p2.items.len(),
                p2.has_more,
                p2.items.iter().map(|i| i.content_type).collect::<Vec<_>>()
            );
            assert_eq!(overlap, 0, "tab={tab} 翻页不应回已见条目");
        }
    }

    /// cr=4 响应的 cell 结构全量 dump。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_recommend_tab_cells() {
        let env = anon_env();
        let (cfg, page) = fetch_tab_first_page("36", &env).await.expect("tab36 cr=4");
        println!(
            "[cells] 解析到的配置: cell_id={} bookstore_id={} 首批 {} 条",
            cfg.cell_id,
            cfg.bookstore_id,
            page.items.len()
        );
        let q: Vec<(String, String)> = [
            ("auth_aweme", "true"),
            ("auth_backward", "true"),
            ("bottom_tab_type", "7"),
            ("client_req_type", "4"),
            ("device_level", "3"),
            ("disable_digg_stat", "false"),
            ("has_video_cache", "false"),
            ("is_horizontal_screen", "false"),
            ("landing_bottom_tab_type", "7"),
            ("last_tab_index", "0"),
            ("last_tab_type", "0"),
            ("offset", "0"),
            ("req_rank_category_id", "0"),
            ("screen_width_px", "1078"),
            (
                "stream_count",
                r#"[{"scene":"1","StreamCount":1,"StreamType":"1"}]"#,
            ),
            ("tab_index", "0"),
            ("tab_type", "36"),
            ("video_type_preferences_str", "[]"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(
            LQ_API_ORIGIN,
            "/reading/bookapi/bookmall/tab/v",
            None,
            &q,
            &env,
        )
        .await
        .expect("cr=4");
        let v: Value = serde_json::from_slice(&bytes).expect("JSON");
        let item = v
            .pointer("/data/tab_item")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter().find(|t| {
                    t.get("tab_type")
                        .and_then(Value::as_i64)
                        .map(|n| n.to_string())
                        .as_deref()
                        == Some("36")
                })
            })
            .expect("tab_item");
        let flex = |x: Option<&Value>| -> String {
            match x {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => "-".into(),
            }
        };
        println!(
            "[cells] tab_item 顶层: bookstore_id={} session={}",
            flex(item.get("bookstore_id")),
            &flex(item.get("session_id"))[..20.min(flex(item.get("session_id")).len())]
        );
        for (i, cell) in item
            .get("cell_data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let nested = cell.get("cell_data").and_then(Value::as_array);
            let n_items: usize = nested
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.get("video_data").and_then(Value::as_array))
                        .map(|a| a.len())
                        .sum()
                })
                .unwrap_or(0);
            let own_items = cell
                .get("video_data")
                .and_then(Value::as_array)
                .map(|a| a.len());
            println!(
                "[cells] cell_data[{i}]: cell_id={} cell_id_str={} cell_name={:?} show_type={} 嵌套cell={} 嵌套条目={n_items} 自带video_data={:?}",
                flex(cell.get("cell_id")),
                flex(cell.get("cell_id_str")),
                flex(cell.get("cell_name")),
                flex(cell.get("show_type")),
                nested.map(Vec::len).unwrap_or(0),
                own_items
            );
            for (j, c) in nested
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                println!(
                    "[cells]   [{i}.{j}] cell_id={} cell_name={:?} show_type={} items={}",
                    flex(c.get("cell_id_str")),
                    flex(c.get("cell_name")),
                    flex(c.get("show_type")),
                    c.get("video_data")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(0)
                );
            }
        }
    }
}
