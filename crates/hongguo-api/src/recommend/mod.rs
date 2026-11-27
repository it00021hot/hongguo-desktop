//! 首页推荐流接口（书城换一换；详见 docs/hongguo-api-endpoints.md）。

mod cache;
mod model;
mod parse;
pub(crate) use parse::recommend_feed_entry;
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
use parse::{is_follow_video_cell, parse_feed_cell_items, parse_recommend_feed};
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
            && page.has_more
            && !page.session_id.is_empty()
            && let Ok(fill) =
                recommend_cell_change(tab, &cfg, &page.session_id, page.next_offset, &[], env).await
        {
            page.items = fill.items;
            page.has_more = fill.has_more;
            page.next_offset = fill.next_offset;
            if !fill.session_id.is_empty() {
                page.session_id = fill.session_id;
            }
        }
        if !cfg.cell_id.is_empty() {
            cache_tab_config_insert(tab, cfg);
        }
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

/// 推荐 tab 也是获取完整 `tab_item[]` 顶栏列表的入口。
const RECOMMEND_TAB_TYPE: &str = "16";
const FOLLOW_TAB_TYPE: &str = "45";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstPageMode {
    ColdPreload,
    ChannelSwitch,
}

impl FirstPageMode {
    fn for_tab(tab: &str) -> Self {
        match tab {
            RECOMMEND_TAB_TYPE | FOLLOW_TAB_TYPE | "36" | "39" => Self::ChannelSwitch,
            _ => Self::ColdPreload,
        }
    }

    fn origin(self) -> &'static str {
        match self {
            Self::ColdPreload => SINFONLINEC_API_ORIGIN,
            Self::ChannelSwitch => LQ_API_ORIGIN,
        }
    }
}

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
    let value = fetched
        .or_else(|| known_tab_config(tab))
        .ok_or_else(|| AppError::Media(format!("推荐 tab 配置不可用: {tab}")))?;
    cache_tab_config_insert(tab, value.clone());
    Ok(value)
}

/// `GET bookmall/tab/v?client_req_type=4`（lq 域）：顶栏列表和切频道共用。
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
    let value = match FirstPageMode::for_tab(tab) {
        // APK 中 1028 是预载刷新，可能只回频道元数据；进入指定频道用切换请求。
        FirstPageMode::ChannelSwitch => request_bookstore_tab(tab, env).await?,
        FirstPageMode::ColdPreload => request_feed_tab_first_page(tab, env).await?,
    };
    parse_tab_first_page(tab, &value)
}

fn parse_tab_first_page(
    tab: &str,
    value: &Value,
) -> AppResult<(RecommendTabConfig, super::discover::FeedPage)> {
    let tabs = value
        .pointer("/data/tab_item")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("推荐 tab 响应缺 tab_item".into()))?;
    let item = tabs
        .iter()
        .find(|t| flex_str(t.get("tab_type")).as_deref() == Some(tab))
        .ok_or_else(|| AppError::Media(format!("推荐 tab 响应无 tab={tab}")))?;
    let cells = item
        .get("cell_data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let following = tab == FOLLOW_TAB_TYPE;
    // 关注只消费作品 cell，不把同页的推荐关注卡片视为关注作品。
    let (feed_cell, items) = if following {
        let cell = cells.iter().find(|cell| is_follow_video_cell(cell));
        (
            cell,
            parse_feed_cell_items(cell.and_then(|cell| cell.get("cell_data")), true),
        )
    } else if let Some(cell) = cells.iter().find(|cell| {
        // APK recommendations cell ("猜你喜欢"). An earlier populated
        // promotion cell is not the stream and its cursor cannot page it.
        flex_str(cell.get("show_type")).as_deref() == Some("407")
    }) {
        (
            Some(cell),
            parse_feed_cell_items(cell.get("cell_data"), false),
        )
    } else {
        // Older responses may not label the feed cell. Only use a cell
        // with actual playable feed entries, never an arbitrary banner.
        cells
            .iter()
            .find_map(|cell| {
                let items = parse_feed_cell_items(cell.get("cell_data"), false);
                (!items.is_empty()).then_some((Some(cell), items))
            })
            .unwrap_or((None, Vec::new()))
    };
    let bookstore_id = config_id(item.get("bookstore_id"))
        .ok_or_else(|| AppError::Media(format!("推荐 tab 配置缺 bookstore_id: tab={tab}")))?;
    let cfg = RecommendTabConfig {
        cell_id: feed_cell
            .and_then(|cell| {
                config_id(cell.get("cell_id_str")).or_else(|| config_id(cell.get("cell_id")))
            })
            .unwrap_or_default(),
        bookstore_id,
    };
    let session_id = feed_cell
        .and_then(|cell| config_id(cell.get("session_id")))
        .or_else(|| config_id(item.get("session_id")))
        .unwrap_or_default();
    // The outer tab's has_more concerns tab content, not the recommendation
    // stream. Its false value must not prevent the first cell/change. A
    // cell-level false or the actual continuation response does end the feed.
    let has_more = !cfg.cell_id.is_empty()
        && !session_id.is_empty()
        && feed_cell
            .and_then(|cell| cell.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
    let offset = item.get("next_offset").and_then(Value::as_i64).unwrap_or(0);
    let next_offset = feed_cell
        .and_then(|cell| cell.get("next_offset"))
        .and_then(Value::as_i64)
        .unwrap_or(offset);
    let page = super::discover::FeedPage {
        session_id,
        has_more,
        next_offset,
        items,
    };
    Ok((cfg, page))
}

fn config_id(value: Option<&Value>) -> Option<String> {
    flex_str(value).filter(|value| !matches!(value.trim(), "" | "0"))
}

fn parse_continuation_page(
    data: Option<&Value>,
    tab: &str,
    session_id: &str,
) -> AppResult<super::discover::FeedPage> {
    let mut page = parse_recommend_feed(data, tab == FOLLOW_TAB_TYPE)?;
    if matches!(page.session_id.trim(), "" | "0") {
        // Some replies do not rotate the session (nor echo it). Preserve
        // the request's cursor context instead of starting again at page one.
        page.session_id = session_id.into();
    }
    Ok(page)
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
    // 翻页与 tab 首页同源：会话是首页响应下发的，
    // 跨域复用同一会话有被拒风险
    let bytes = api_call_reading(
        FirstPageMode::for_tab(tab).origin(),
        RANK_CELL_PATH,
        None,
        &q,
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析推荐流失败: {e}")))?;
    check_code(&value)?;
    parse_continuation_page(value.get("data"), tab, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn channel_entry_and_cold_start_keep_distinct_routes() {
        for tab in ["16", "45", "36", "39"] {
            assert_eq!(FirstPageMode::for_tab(tab), FirstPageMode::ChannelSwitch);
            assert_eq!(FirstPageMode::for_tab(tab).origin(), LQ_API_ORIGIN);
        }
        assert_eq!(FirstPageMode::for_tab("other"), FirstPageMode::ColdPreload);
        assert_eq!(
            FirstPageMode::for_tab("other").origin(),
            SINFONLINEC_API_ORIGIN
        );
    }

    #[test]
    fn channel_parser_picks_requested_feed_and_excludes_banners() {
        let value = json!({"data": {"tab_item": [
            {"tab_type": 16, "bookstore_id": "8", "cell_data": []},
            {"tab_type": 36, "bookstore_id": "2", "session_id": "mock-session", "cell_data": [
                {"cell_id": "5", "cell_data": []},
                {"cell_id": "6", "cell_data": [{"video_data": [
                    {"series_id": "9", "video_detail": {"series_title": "Comic only", "content_type": 1}}
                ]}]}
            ]}
        ]}});
        let (cfg, page) = parse_tab_first_page("36", &value).unwrap();
        assert_eq!(cfg.cell_id, "6");
        assert_eq!(cfg.bookstore_id, "2");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].title, "Comic only");
        assert_eq!(page.session_id, "mock-session");
        assert!(page.has_more);
    }

    #[test]
    fn recommendations_use_the_feed_cell_cursor_even_when_outer_tab_is_finished() {
        let value = json!({"data": {"tab_item": [{
            "tab_type": 16,
            "bookstore_id": "official-plan",
            "session_id": "outer-session",
            "has_more": false,
            "next_offset": 99,
            "cell_data": [
                {"show_type": 583, "cell_id": "promo-cell", "cell_data": [
                    {"video_data": [{"series_id": "901"}]}
                ]},
                {"show_type": "407", "cell_id_str": "official-cell", "session_id": "feed-session",
                 "next_offset": 3, "cell_data": [
                    {"video_data": [{"series_id": "123", "video_detail": {"series_title": "Official"}}]}
                ]}
            ]
        }]}});
        let (cfg, page) = parse_tab_first_page(RECOMMEND_TAB_TYPE, &value).unwrap();
        assert_eq!(cfg.cell_id, "official-cell");
        assert_eq!(cfg.bookstore_id, "official-plan");
        assert_eq!(page.session_id, "feed-session");
        assert_eq!(page.next_offset, 3);
        assert!(page.has_more);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].series_id, "123");

        let mut value = value;
        value["data"]["tab_item"][0]["cell_data"][1]["has_more"] = json!(false);
        assert!(
            !parse_tab_first_page(RECOMMEND_TAB_TYPE, &value)
                .unwrap()
                .1
                .has_more
        );
    }

    #[test]
    fn continuation_keeps_its_session_unless_server_rotates_it() {
        let mut value = json!({"cell_view": {"cell_data": [{"video_data": [
            {"series_id": 42}
        ]}]}, "next_offset": 9, "has_more": true});
        let page = parse_continuation_page(Some(&value), RECOMMEND_TAB_TYPE, "in-use").unwrap();
        assert_eq!(page.session_id, "in-use");
        assert_eq!(page.next_offset, 9);
        assert_eq!(page.items[0].series_id, "42");

        value["session_id"] = json!("rotated");
        value["has_more"] = json!(false);
        let page = parse_continuation_page(Some(&value), RECOMMEND_TAB_TYPE, "in-use").unwrap();
        assert_eq!(page.session_id, "rotated");
        assert!(!page.has_more);
    }

    #[test]
    fn following_uses_the_work_video_not_its_related_album() {
        let post = json!({"series_id": "98", "vid": "99", "cell_extra_data": {
            "rec_filter_info": {"filter_id_map": {"pugc_id": "23", "album_id": "98"}}
        }, "video_detail": {
            "series_title": "Creator's post",
            "related_album_id": 98,
            "ugc_user_info": {"base_info": {"user_name": "Creator"}},
            "bottom_bar": {"text": ["Drama", "10 episodes"]},
            "video_list": [{"series_id": "23", "vid": "24", "content_type": 1004, "cover": "post-cover"}]
        }});
        let value = json!({"data": {"tab_item": [
            {"tab_type": "45", "bookstore_id": 2, "session_id": "fake-session", "has_more": true,
             "next_offset": 300, "cell_data": [
                {"show_type": 647, "cell_id": 6, "cell_data": [{"video_data": [post.clone()]}]},
                {"show_type": 648, "cell_id": 7, "next_offset": 3, "has_more": true, "cell_data": [post.clone()]}
            ]}
        ]}});
        let (cfg, page) = parse_tab_first_page(FOLLOW_TAB_TYPE, &value).unwrap();
        assert_eq!(cfg.cell_id, "7");
        assert!(page.has_more);
        assert_eq!(page.next_offset, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].series_id, "23");
        assert_eq!(page.items[0].vid, "24");
        assert_eq!(page.items[0].content_type, 1004);
        assert_eq!(page.items[0].cover, "post-cover");
        let highlight = page.items[0].highlight.as_ref().unwrap();
        assert_eq!(highlight.author, "Creator");
        let related = highlight.related.as_ref().unwrap();
        assert_eq!(related.series_id, "98");
        assert_eq!(related.kind, hongguo_common::model::RelatedKind::Drama);
        assert_eq!(related.label, "Drama · 10 episodes");

        let more = json!({"cell_view": {"cell_data": [{"show_type": 648, "cell_data": [post]}]},
                          "session_id": "fake-session", "has_more": false, "next_offset": 6});
        let more = parse_recommend_feed(Some(&more), true).unwrap();
        assert_eq!(more.items, page.items);
        assert!(!more.has_more);
    }

    #[test]
    fn an_empty_channel_without_a_feed_cell_has_no_continuation() {
        let value = json!({"data": {"tab_item": [
            {"tab_type": "45", "bookstore_id": 2, "session_id": "mock-session", "cell_data": []}
        ]}});
        let (_, page) = parse_tab_first_page("45", &value).unwrap();
        assert!(page.items.is_empty());
        assert!(!page.has_more);
    }
}
