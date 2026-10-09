//! 首页推荐流接口（书城换一换，hgplayer RecommendTab 同源同流程；
//! 2026-10-08 抓包锁定，详见 docs/hongguo-api-endpoints.md）。

mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use super::rank::RANK_CELL_PATH;
use crate::error::{AppError, AppResult};

pub use model::RecommendTabConfig;

/// 拉一页首页推荐流（书城换一换，hgplayer RecommendTab 同源同流程）。
///
/// tab 表（bookmall/tab 下发）：推荐 16 / 漫剧 36 / 真人剧 39 / 关注 45
/// （关注是登录后的追更流，不在这条数据面里）。生产链路里 tab 由前端
/// 字面量直传（与旧 landpage genre 直传同一模式）。
///
/// 抓包口径（2026-10-08，hgplayer 1.1.6 实操锁定，两段式）：
/// - **首页**：`GET bookmall/tab/v?client_req_type=4&tab_type=<T>`——目标
///   tab_item 自带 `bookstore_id` + `cell_id`（⚠️ **每 tab 不同**，用推荐位
///   的 id 打 tab39 服务端回落混合池、清一色 1004 漫剧——「真人 tab 全是
///   漫剧」的根因）与会话 `session_id`，首批条目嵌在
///   `cell_data[0].cell_data[].video_data[]`；
/// - **翻页**：`GET cell/change/v`，双 id 用该 tab 自己的 + `session_id`
///   （首页下发）+ **`filter_ids`=已下发过的 series_id 逗号表**（服务端
///   排除已见）+ `offset=0`（hgplayer 恒 0，不用响应的 next_offset）；
/// - 内容按时间桶在服务端轮换，「换一批」= 新会话重拉（filter_ids 清空），
///   不做客户端偏移。
pub async fn fetch_recommend_feed(
    tab: &str,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    env: &ApiEnv,
) -> AppResult<super::discover::FeedPage> {
    if session_id.is_empty() {
        let (cfg, mut page) = fetch_tab_first_page(tab, env).await?;
        // 首批偶发空批：带着 cr=4 下发的会话直接走 cell/change 补拉
        if page.items.is_empty()
            && !page.session_id.is_empty()
            && let Ok(fill) = recommend_cell_change(tab, &cfg, &page.session_id, 0, &[], env).await
        {
            page.items = fill.items;
            page.has_more = fill.has_more;
            page.next_offset = fill.next_offset;
        }
        cache_tab_config_insert(tab, cfg);
        Ok(page)
    } else {
        let cfg = resolve_tab_config(tab, env).await?;
        recommend_cell_change(tab, &cfg, session_id, offset, filter_ids, env).await
    }
}

/// 抓包已知的每 tab 配置（2026-10-08，hgplayer 与本机设备取值一致——
/// 服务端按 tab 下发的常量）。`fetch_tab_first_page` 失败时的兜底，
/// 供翻页继续可用（首页已成功过的会话不依赖它）。
fn known_tab_config(tab: &str) -> Option<RecommendTabConfig> {
    let (cell_id, bookstore_id) = match tab {
        "16" => ("7294257141911650341", "7294257258253254693"),
        "36" => ("7597343008509411390", "7597343072904560702"),
        "39" => ("7650476176527343678", "7650476689721409598"),
        _ => return None,
    };
    Some(RecommendTabConfig {
        cell_id: cell_id.into(),
        bookstore_id: bookstore_id.into(),
    })
}

/// 配置进程内缓存（首页 cr=4 与翻页共享；失败回落抓包已知值）。
type TabConfigCache = std::sync::Mutex<std::collections::HashMap<String, RecommendTabConfig>>;
static TAB_CONFIG_CACHE: std::sync::OnceLock<TabConfigCache> = std::sync::OnceLock::new();

fn cache_tab_config_insert(tab: &str, cfg: RecommendTabConfig) {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .unwrap()
        .insert(tab.to_string(), cfg);
}

fn cached_tab_config(tab: &str) -> Option<RecommendTabConfig> {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .unwrap()
        .get(tab)
        .cloned()
}

/// 解析 tab 的 cell 配置：优先进程缓存，否则现场取（bookmall/tab cr=4），
/// 再不行回落抓包已知值（同样缓存住，免得每页都重试配置请求）。
async fn resolve_tab_config(tab: &str, env: &ApiEnv) -> AppResult<RecommendTabConfig> {
    if let Some(v) = cached_tab_config(tab) {
        return Ok(v);
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

/// `GET bookmall/tab/v?client_req_type=4&tab_type=<T>` →（配置, 首页）。
///
/// hgplayer 切 tab 同款请求（2026-10-08 抓包锁定）。注意 tab_item 里的
/// id 字段是 JSON 数字，容忍字符串形态。
async fn fetch_tab_first_page(
    tab: &str,
    env: &ApiEnv,
) -> AppResult<(RecommendTabConfig, super::discover::FeedPage)> {
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
    let flex_str = |v: Option<&Value>| -> Option<String> {
        match v {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    };
    let tabs = value
        .pointer("/data/tab_item")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("推荐 tab 响应缺 tab_item".into()))?;
    let item = tabs
        .iter()
        .find(|t| flex_str(t.get("tab_type")).as_deref() == Some(tab))
        .ok_or_else(|| AppError::Media(format!("推荐 tab 响应无 tab={tab}")))?;
    // cell_data[] 里有多个 cell：推荐流是嵌套 cell_data[].video_data[] 非空的
    // 那个（cell_name=「猜你喜欢」，show_type=407）；[0] 可能是空的金刚位
    // 运营卡（show_type=583，tab36 实测）——不能按位置取，按内容取。
    // bookstore_id 在 tab_item 顶层。
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
        // 第一个带条目的 cell 即推荐流 cell；后续 cell（换一换位等）不取
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
        // 没有任何带条目的 cell：配置退回 [0] 的 id（空金刚位场景），条目为空
        // 由调用方的补拉分支兜底
        cell_id = flex_str(item.pointer("/cell_data/0/cell_id_str"))
            .or_else(|| flex_str(item.pointer("/cell_data/0/cell_id")))
            .unwrap_or_default();
    }
    let cfg = RecommendTabConfig {
        cell_id,
        bookstore_id,
    };
    // tab 级 has_more 是 tab 列表语义（恒 false），流的分页真值由翻页的
    // cell/change 给——这里乐观置 true，翻到空页自然停；首批无 offset，
    // 第一次翻页 offset=0（hgplayer 同款，之后用响应 next_offset 递进）
    let page = super::discover::FeedPage {
        session_id: flex_str(item.get("session_id")).unwrap_or_default(),
        has_more: true,
        next_offset: 0,
        items,
    };
    Ok((cfg, page))
}

/// 翻页：`GET cell/change/v`（hgplayer 同款：session + filter_ids 排除已见 +
/// `offset=上一页响应的 next_offset`（0→6→12 递进，抓包实锤）；
/// 双 id 用目标 tab 自己的）。
async fn recommend_cell_change(
    tab: &str,
    cfg: &RecommendTabConfig,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    env: &ApiEnv,
) -> AppResult<super::discover::FeedPage> {
    let mut q: Vec<(String, String)> = [
        ("bottom_tab_type", "7"),
        ("category_id", "0"),
        ("cell_id", cfg.cell_id.as_str()),
        ("cell_sub_id", "0"),
        ("change_type", "0"),
        ("client_req_type", "2"),
        ("client_template", "7"),
        ("limit", "0"),
        ("offset", &offset.to_string()),
        ("plan_id", cfg.bookstore_id.as_str()),
        ("tab_type", tab),
        ("video_type_preferences_str", "[]"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if !session_id.is_empty() {
        q.push(("session_id".to_string(), session_id.to_string()));
    }
    if !filter_ids.is_empty() {
        q.push(("filter_ids".to_string(), filter_ids.join(",")));
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析推荐流失败: {e}")))?;
    check_code(&value)?;
    parse_recommend_feed(value.get("data"))
}

/// cell/change 响应 → FeedPage。条目在 `cell_view.cell_data[].video_data[]`，
/// 身份字段在条目的 `video_detail`（series_title/series_cover/episode_cnt/
/// category_schema…），与 landpage 的 title/cover 族不同名。
fn parse_recommend_feed(data: Option<&Value>) -> AppResult<super::discover::FeedPage> {
    use super::discover::FeedPage;
    let data = data.ok_or_else(|| AppError::Media("推荐流响应缺少 data".into()))?;
    let mut items = Vec::new();
    for cell in data
        .pointer("/cell_view/cell_data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        for raw in cell
            .get("video_data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(it) = recommend_feed_entry(raw) {
                items.push(it);
            }
        }
    }
    Ok(FeedPage {
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        session_id: data
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items,
    })
}

/// 一条推荐流条目（cell/change 与 bookmall/tab 嵌套 cell 共用同一形态）。
fn recommend_feed_entry(raw: &Value) -> Option<super::discover::FeedItem> {
    let det = raw.get("video_detail");
    // series_id 双形态：条目顶层数字（JSON number 直接 to_string 无损）
    // 与 video_detail.series_id_str，前者在则用前者
    let series_id = raw
        .get("series_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            raw.get("series_id")
                .and_then(Value::as_i64)
                .map(|n| n.to_string())
        })
        .or_else(|| {
            det.and_then(|d| d.get("series_id_str"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if series_id.is_empty() {
        return None;
    }
    let season_tag = det
        .and_then(|d| d.get("secondary_infos"))
        .and_then(Value::as_array)
        .and_then(|arr| {
            // 季徽：data_type 缺省或 0 的第一条（「第1季」，highlight）
            arr.iter().find_map(|it| {
                let dt = it.get("data_type").and_then(Value::as_i64);
                (dt.is_none() || dt == Some(0))
                    .then(|| it.get("content").and_then(Value::as_str))
                    .flatten()
                    .map(str::to_string)
            })
        })
        .unwrap_or_default();
    Some(super::discover::FeedItem {
        series_id,
        title: det
            .and_then(|d| d.get("series_title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover: det
            .and_then(|d| d.get("series_cover"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                det.and_then(|d| d.get("series_cover_uri"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default()
            .to_string(),
        horiz_cover: String::new(),
        vid: raw
            .get("vid")
            .map(|v| match v {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        episode_cnt: det
            .and_then(|d| d.get("episode_cnt"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as u32,
        play_cnt: det
            .and_then(|d| d.get("series_play_cnt"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        comment_count: 0,
        // 评分（secondary_infos data_type=4，「9.6分」形态；无则 0）
        score: det
            .and_then(|d| d.get("secondary_infos"))
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|it| {
                    (it.get("data_type").and_then(Value::as_i64) == Some(4))
                        .then(|| it.get("content").and_then(Value::as_str))
                        .flatten()
                        .and_then(|s| s.trim_end_matches('分').parse::<f64>().ok())
                })
            })
            .unwrap_or(0.0),
        tags: super::discover::parse_tags(det.and_then(|d| d.get("category_schema"))),
        season_tag,
        // 热度行（火焰图标 + 文案，hgplayer 同款展示）：条目级 rec_tags 里
        // data_type=1 的 content——「共217万人在追」「逆袭榜 No.3」「真人剧
        // 新番榜 No.4」等，多段全部拼接（2026-10-08 抓包实锤）
        heat_text: raw
            .get("rec_tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter(|t| t.get("data_type").and_then(Value::as_i64) == Some(1))
                    .filter_map(|t| t.get("content").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default(),
        // 运营角标 pill（「新剧」等，官方随 tag_info.bg_color 下发颜色，
        // 前端用主题色即可）
        badge: det
            .and_then(|d| d.pointer("/tag_info/text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        content_type: det
            .and_then(|d| d.get("content_type"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 抓包样本形状（2026-10-08 probe_recommend_tab_feed 直连实录）：
    /// 条目身份在 video_detail，series_id 顶层是数字、季徽的 data_type 缺省。
    #[test]
    fn parses_recommend_feed_entries() {
        let data: Value = serde_json::json!({
            "has_more": true,
            "next_offset": 6,
            "session_id": "20261008181149086C4BCA8151179AD830",
            "cell_view": {
                "cell_data": [
                    { "video_data": [{
                        "series_id": 7677187660368055321_i64,
                        "vid": 7677203040796953624_i64,
                        "rec_tags": [
                            { "content": "真人剧新番榜 No.4", "data_type": 1 },
                            { "content": "4920万热度", "data_type": 1 },
                            { "content": "不展示的杂项", "data_type": 9 }
                        ],
                        "video_detail": {
                            "series_id_str": "7677187660368055321",
                            "series_title": "谁把我仙侠游戏退出键扣了3D版",
                            "series_cover": "",
                            "series_cover_uri": "https://p3-sign.douyinpic.com/x~tplv.image",
                            "episode_cnt": 248,
                            "series_play_cnt": 17050000,
                            "content_type": 1004,
                            "category_schema": "[{\"category_id\":755,\"name\":\"脑洞\"},{\"category_id\":37,\"name\":\"穿越\"}]",
                            "tag_info": { "text": "新剧", "bg_color": ["#00AE83"] },
                            "secondary_infos": [
                                { "content": "第1季", "highlight": true },
                                { "content": "9.6分", "data_type": 4 },
                                { "content": "异界", "data_type": 3 }
                            ]
                        }
                    }]},
                    { "video_data": [{ "vid": 123_i64, "video_detail": { "series_title": "缺 id 的废条目" } }] }
                ]
            }
        });
        let page = parse_recommend_feed(Some(&data)).unwrap();
        assert_eq!(page.items.len(), 1, "缺 series_id 的条目要跳过");
        let it = &page.items[0];
        assert_eq!(
            it.series_id, "7677187660368055321",
            "顶层数字 series_id 无损转字符串"
        );
        assert_eq!(it.title, "谁把我仙侠游戏退出键扣了3D版");
        assert_eq!(it.vid, "7677203040796953624");
        // series_cover 空串时回落 series_cover_uri
        assert!(it.cover.contains("douyinpic"));
        assert_eq!(it.episode_cnt, 248);
        assert_eq!(it.play_cnt, 17_050_000);
        assert_eq!(it.content_type, 1004);
        assert_eq!(it.tags, vec!["脑洞", "穿越"]);
        assert_eq!(
            it.season_tag, "第1季",
            "data_type 缺省的 secondary_info 是季徽"
        );
        assert_eq!(
            it.heat_text, "真人剧新番榜 No.4 · 4920万热度",
            "热度行 = rec_tags data_type=1 拼接"
        );
        assert_eq!(it.badge, "新剧", "运营角标 = video_detail.tag_info.text");
        assert_eq!(it.score, 9.6, "评分 = secondary data_type=4 去掉「分」");
        assert!(page.has_more);
        assert_eq!(page.next_offset, 6);
        assert_eq!(page.session_id, "20261008181149086C4BCA8151179AD830");
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use crate::domain::api::rank::probe::anon_env;

    /// 首页推荐流两段式全链路（2026-10-08 抓 hgplayer 切「真人剧」tab +
    /// 滚动翻页实锤）：首页 `bookmall/tab cr=4`（每 tab 自带 bookstore_id/
    /// cell_id + 会话 + 嵌套首批），翻页 `cell/change`（双 id + session +
    /// filter_ids 排除已见 + offset 恒 0）。验证各 tab 类型过滤（39 全
    /// ct=1 真人剧）与翻页不重复。
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
            // filter_ids 排除已见是翻页语义的核心：服务端必须避开它
            assert_eq!(overlap, 0, "tab={tab} 翻页不应回已见条目");
        }
    }

    /// cr=4 响应的 cell 结构全量 dump（tab36 实测含多个 cell、不同
    /// cell_id——hgplayer 翻页用的是与 bookstore_id 同前缀的那个）。
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
        // 直接打原始请求 dump cell_data[*] 全量结构
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
