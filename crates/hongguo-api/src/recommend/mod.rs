//! 首页推荐流接口（书城换一换；2026-10-08 抓包锁定，2026-10-25 冷启动
//! 全参数复对齐，详见 docs/hongguo-api-endpoints.md）。

mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use super::rank::RANK_CELL_PATH;
use hongguo_common::{AppError, AppResult};

pub use model::{FeedTabMeta, RecommendTabConfig};

/// 拉一页首页推荐流（书城换一换）。
///
/// tab 表（bookmall/tab 下发）：推荐 16 / 漫剧 36 / 真人剧 39 / 关注 45
/// （关注是登录后的追更流，不在这条数据面里）。生产链路里 tab 由前端
/// 字面量直传（与旧 landpage genre 直传同一模式）。
///
/// 抓包口径（2026-10-08 实操锁定，两段式）：
/// - **首页**：`GET bookmall/tab/v?client_req_type=4&tab_type=<T>`——目标
///   tab_item 自带 `bookstore_id` + `cell_id`（⚠️ **每 tab 不同**，用推荐位
///   的 id 打 tab39 服务端回落混合池、清一色 1004 漫剧——「真人 tab 全是
///   漫剧」的根因）与会话 `session_id`，首批条目嵌在
///   `cell_data[0].cell_data[].video_data[]`；
/// - **翻页**：`GET cell/change/v`，双 id 用该 tab 自己的 + `session_id`
///   （首页下发）+ **`filter_ids`=已下发过的 series_id 逗号表**（服务端
///   排除已见）+ `offset=上一页响应的 next_offset`（0→6→12 递进）；
/// - 内容按时间桶在服务端轮换，「换一批」= 新会话重拉（filter_ids 清空），
///   不做客户端偏移。
pub async fn fetch_recommend_feed(
    tab: &str,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    env: &ApiEnv,
) -> AppResult<super::discover::FeedPage> {
    let page = if session_id.is_empty() {
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

/// 抓包已知的每 tab 配置（2026-10-08 抓包锁定——服务端按 tab 下发的常量）。`fetch_tab_first_page` 失败时的兜底，
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

/// 推荐 tab 在顶栏里的 tab_type：顶栏列表请求的源 tab（其响应 `tab_item[]`
/// 即完整顶栏），也是种子语义的归属 tab（「你的内容」只进推荐 tab——壳侧口径）。
const RECOMMEND_TAB_TYPE: &str = "16";

/// 顶栏 tab 列表（服务端下发）。
///
/// 一次 cr=4 请求（推荐 tab）的 `tab_item[]` 就是完整顶栏：tab_type/title/
/// 顺序即官方顶栏顺序，线格式见 [`FeedTabMeta`]（2026-10-15 官方抓包实锤；
/// 非活跃 tab 的 bookstore_id 为 null，切换到该 tab 时它自己的 cr=4 才带）。
/// 进程缓存按 [`TAB_CONFIG_TTL`] 节奏复验，重取失败沿用旧列表
/// （stale-while-error）——运营调整晚一刻到达，好过顶栏整体报错。
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
    cache_feed_tabs_insert(tabs.clone());
    Ok(tabs)
}

/// tab_item[] → 顶栏列表（保序）。缺 tab_type 的残缺条目跳过。
fn parse_feed_tabs(value: &Value) -> AppResult<Vec<FeedTabMeta>> {
    let tabs = value
        .pointer("/data/tab_item")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("推荐 tab 响应缺 tab_item".into()))?;
    Ok(tabs
        .iter()
        .filter_map(|t| {
            let tab_type = flex_str(t.get("tab_type"))?;
            Some(FeedTabMeta {
                tab_type,
                title: t
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                bookstore_id: flex_str(t.get("bookstore_id")).filter(|s| !s.is_empty()),
            })
        })
        .collect())
}

/// 顶栏列表缓存（与 tab 配置同 TTL；stale-while-error）。
#[derive(Clone)]
struct CachedFeedTabs {
    tabs: Vec<FeedTabMeta>,
    at: std::time::Instant,
}

type FeedTabsCache = parking_lot::Mutex<Option<CachedFeedTabs>>;
static FEED_TABS_CACHE: std::sync::OnceLock<FeedTabsCache> = std::sync::OnceLock::new();

fn cache_feed_tabs_insert(tabs: Vec<FeedTabMeta>) {
    *FEED_TABS_CACHE.get_or_init(FeedTabsCache::default).lock() = Some(CachedFeedTabs {
        tabs,
        at: std::time::Instant::now(),
    });
}

fn cached_feed_tabs() -> Option<CachedFeedTabs> {
    FEED_TABS_CACHE
        .get_or_init(FeedTabsCache::default)
        .lock()
        .clone()
}

/// 配置进程内缓存（首页 cr=4 与翻页共享；失败回落抓包已知值）。
///
/// 条目带时间戳做 TTL 过期：cell_id/bookstore_id 原本
/// 按服务端常量假设永不重取，运营调整后只能靠重启恢复。现在超过
/// [`TAB_CONFIG_TTL`] 后下次使用会先重取一次，重取失败继续用旧值
/// （stale-while-error）——翻页不会因此重打配置请求。
#[derive(Clone)]
struct CachedTabConfig {
    cfg: RecommendTabConfig,
    at: std::time::Instant,
}

/// 配置复验间隔：服务端常量假设下 1 小时重验一次足够及时，开销可忽略。
const TAB_CONFIG_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

type TabConfigCache = parking_lot::Mutex<std::collections::HashMap<String, CachedTabConfig>>;
static TAB_CONFIG_CACHE: std::sync::OnceLock<TabConfigCache> = std::sync::OnceLock::new();

fn cache_tab_config_insert(tab: &str, cfg: RecommendTabConfig) {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .insert(
            tab.to_string(),
            CachedTabConfig {
                cfg,
                at: std::time::Instant::now(),
            },
        );
}

fn cached_tab_config(tab: &str) -> Option<CachedTabConfig> {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .get(tab)
        .cloned()
}

/// 解析 tab 的 cell 配置：进程缓存命中且未过期直接用；过期先重取一次，
/// 重取失败沿用旧值（stale-while-error）。无缓存时现场取（bookmall/tab
/// cr=4），再不行回落抓包已知值（同样缓存住，免得每页都重试配置请求）。
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

/// `GET bookmall/tab/v?client_req_type=4&tab_type=<T>` 并过业务 code 检查；
/// tab 首页与顶栏列表走同一请求。
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

/// tab_item 字段数字/字符串双形态，容忍后归一为字符串。
fn flex_str(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// `GET bookmall/tab/v?client_req_type=4&tab_type=<T>` →（配置, 首页）。
///
/// 切 tab 请求（2026-10-08 抓包锁定）。注意 tab_item 里的
/// id 字段是 JSON 数字，容忍字符串形态。
async fn fetch_tab_first_page(
    tab: &str,
    env: &ApiEnv,
) -> AppResult<(RecommendTabConfig, super::discover::FeedPage)> {
    let value = request_bookstore_tab(tab, env).await?;
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
    // 第一次翻页 offset=0，之后用响应 next_offset 递进
    let page = super::discover::FeedPage {
        session_id: flex_str(item.get("session_id")).unwrap_or_default(),
        has_more: true,
        next_offset: 0,
        items,
    };
    Ok((cfg, page))
}

// ---------------------------------------------------------------- 会话与曝光上下文

/// 官方 cell/change 的图片缩略规格（base64(JSON)，2026-10-25 冷启动抓包
/// 逐字节对齐：360 中图 / 屏宽大图 / 99 小图——宽度跟随设备屏宽，本仓
/// 档案 1078 与 tab/v 同源）。
const FEED_IMAGE_SHRINK: &str = "W3siaW1hZ2VfdHlwZSI6MiwiaW1hZ2Vfd2lkdGgiOjM2MCwic2hyaW5rX3R5cGUiOjJ9LHsiaW1hZ2VfdHlwZSI6MywiaW1hZ2Vfd2lkdGgiOjEwNzgsInNocmlua190eXBlIjozfSx7ImltYWdlX3R5cGUiOjQsImltYWdlX3dpZHRoIjo5OSwic2hyaW5rX3R5cGUiOjR9XQ==";

/// 官方 cell/change 的推荐补充上下文：base64 即
/// `{"recent_dislike_gid":[],"session_app_stay_time":0}`（2026-10-25 抓包）。
const RECOMMEND_EXTRA: &str =
    "eyJyZWNlbnRfZGlzbGlrZV9naWQiOltdLCJzZXNzaW9uX2FwcF9zdGF5X3RpbWUiOjB9";

/// NPS 插入位配额（官方恒值，2026-10-25 抓包）。
const NPS_INSERT_RANK_JSON: &str = r#"{"1":3,"3":3,"2":3,"4":3}"#;

/// 进程生命周期的会话 UUID。官方每次启动发三枚 uuid 作 session_uuid /
/// normal_session_id / cold_start_session_id；本仓用同一枚派生两形态
/// （裸 uuid 与 `uuid#1`）——服务端只要求同会话内一致。
fn session_uuid() -> &'static str {
    static UUID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    UUID.get_or_init(uuid_v4)
}

/// 随机 UUID v4（不为此引入 uuid crate，rand 随机字节拼 8-4-4-4-12）。
fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = b[6] & 0x0f | 0x40; // version 4
    b[8] = b[8] & 0x3f | 0x80; // variant 10xx
    let hex = |range: std::ops::Range<usize>| {
        b[range]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

/// 一条已下发条目的曝光记录：响应卡片 `cell_extra_data.rec_filter_info`
/// 的原样保留（content_type 字符串与 filter_id_map 不做映射猜测）。
#[derive(Clone)]
struct RecImpression {
    content_type: String,
    rec_group_id: i64,
    filter_id_map: serde_json::Value,
}

/// 最近曝光环容量：官方 latest_impression_gids 一次 2~6 个，取 8 留余量。
const IMPRESSION_CAP: usize = 8;

type ImpressionRing = parking_lot::Mutex<Vec<RecImpression>>;
static RECENT_IMPRESSIONS: std::sync::OnceLock<ImpressionRing> = std::sync::OnceLock::new();

/// 记录一批曝光（翻页请求把它原样回传给服务端做个性化去重上下文）。
fn push_impressions(items: impl IntoIterator<Item = RecImpression>) {
    let ring = RECENT_IMPRESSIONS.get_or_init(|| parking_lot::Mutex::new(Vec::new()));
    let mut guard = ring.lock();
    for item in items {
        if guard.iter().any(|i| i.rec_group_id == item.rec_group_id) {
            continue;
        }
        guard.push(item);
    }
    let excess = guard.len().saturating_sub(IMPRESSION_CAP);
    if excess > 0 {
        guard.drain(0..excess);
    }
}

fn recent_impressions() -> Vec<RecImpression> {
    RECENT_IMPRESSIONS
        .get_or_init(|| parking_lot::Mutex::new(Vec::new()))
        .lock()
        .clone()
}

/// 曝光记录 → `rec_filter_info_list` 线格式（官方键序 content_type /
/// filter_id_map / rec_group_id，rec_group_id 是数字）。
fn rec_filter_info_list(items: &[RecImpression]) -> serde_json::Value {
    serde_json::Value::Array(
        items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "content_type": i.content_type,
                    "filter_id_map": i.filter_id_map,
                    "rec_group_id": i.rec_group_id,
                })
            })
            .collect(),
    )
}

/// 从响应卡片提取曝光记录；卡片不带 `cell_extra_data.rec_filter_info`
/// （部分运营卡）则返回 None，不猜测类型映射。
fn rec_impression_of(raw: &Value) -> Option<RecImpression> {
    let info = raw.pointer("/cell_extra_data/rec_filter_info")?;
    let content_type = info
        .get("content_type")
        .and_then(Value::as_str)?
        .to_string();
    let rec_group_id = info.get("rec_group_id").and_then(Value::as_i64)?;
    Some(RecImpression {
        content_type,
        rec_group_id,
        filter_id_map: info.get("filter_id_map").cloned().unwrap_or(Value::Null),
    })
}

/// 翻页：`GET cell/change/v`（session + filter_ids 排除已见 +
/// `offset=上一页响应的 next_offset`（0→6→12 递进，抓包实锤）；
/// 双 id 用目标 tab 自己的）。
///
/// 参数表按官方 App 7.3.9.32 实流逐字段对齐（2026-10-25 冷启动抓包：
/// 业务零值族 + 会话状态族 + 个性化上下文三部分都在参）。
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
        // —— 业务零值族（官方恒 0/false，缺失会被当无个性化上下文处理）
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
        // —— 会话状态族（官方客户端的运行态快照，服务端按存在性个性化）
        ("app_dark_mode", "1"),
        ("app_launch_times", "0"),
        ("app_mini_window", "0"),
        ("battery_pct", "71"),
        ("charging", "0"),
        ("cold_start_session_cnt_in_day", "4"),
        ("cold_start_session_cnt_in_life", "32"),
        ("cold_start_session_id", uuid),
        ("current_volume", "0"),
        ("down_speed", "60000"),
        ("font_scale", "100"),
        ("gender", "2"),
        ("har_status", "0"),
        ("is_power_save_mode", "0"),
        ("network_type", "4"),
        ("normal_session_cnt_in_day", "8"),
        ("normal_session_cnt_in_life", "64"),
        ("normal_session_id", &format!("{uuid}#1")),
        ("rom_version", "V417IR release-keys"),
        ("screen_brightness", "21"),
        ("sys_dark_mode", "1"),
        ("sys_mini_window", "1"),
        // —— 排除与去重（翻页语义核心）
        ("session_id", session_id),
        ("filter_ids", &filter_ids.join(",")),
    ]
    .into_iter()
    // 空会话/空排除表不发对应键（与调用方约定一致：首页会话还没建立）
    .filter(|(k, v)| !((*k == "session_id" || *k == "filter_ids") && v.is_empty()))
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
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
    // 卡片自带的曝光上下文进最近曝光环（翻页 rec_filter_info_list 的数据源）
    if let Some(impression) = rec_impression_of(raw) {
        push_impressions(std::iter::once(impression));
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
        // 推荐位 vid 两个位置都见过：顶层 vid（2026-10-08 样本）与
        // video_detail.video_list[0].vid（2026-10-14 实流，起点集）——
        // 批量取流预取的 vid 源就是它，两级回落兜住
        vid: raw
            .get("vid")
            .map(|v| match v {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .or_else(|| {
                det.and_then(|d| d.get("video_list"))
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("vid"))
                    .map(|v| match v {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        _ => String::new(),
                    })
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
        // 热度行（火焰图标 + 文案）：条目级 rec_tags 里
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

    /// 顶栏列表解析（2026-10-15 官方实抓样本形状）：顺序即顶栏顺序
    /// 关注/漫剧/真人剧/推荐，仅活跃 tab（16）带 bookstore_id，tab_type
    /// 数字/字符串双形态，缺 tab_type 的残缺条目跳过。
    #[test]
    fn parses_feed_tab_bar() {
        let value = serde_json::json!({
            "data": { "tab_item": [
                { "tab_type": 45, "title": "关注" },
                { "tab_type": 36, "title": "漫剧", "bookstore_id": null },
                { "tab_type": 39, "title": "真人剧" },
                { "title": "缺 tab_type 的废条目" },
                { "tab_type": "16", "title": "推荐", "bookstore_id": "7294257258253254693" }
            ]}
        });
        let tabs = parse_feed_tabs(&value).unwrap();
        assert_eq!(
            tabs.iter().map(|t| t.tab_type.as_str()).collect::<Vec<_>>(),
            ["45", "36", "39", "16"],
            "保序且跳过残缺条目"
        );
        assert_eq!(tabs[0].title, "关注");
        assert_eq!(tabs[0].bookstore_id, None, "游客态关注无数据面");
        assert_eq!(tabs[1].bookstore_id, None);
        assert_eq!(tabs[3].title, "推荐");
        assert_eq!(tabs[3].bookstore_id.as_deref(), Some("7294257258253254693"));
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use crate::rank::probe::anon_env;

    /// 顶栏列表直连实测（2026-10-15 官方抓包：游客拿到 45/36/39/16 四个
    /// tab，顺序即官方顶栏，仅 16 带 bookstore_id）。
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

    /// 首页推荐流两段式全链路（2026-10-08 抓官方切「真人剧」tab +
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
    /// cell_id——翻页用的是与 bookstore_id 同前缀的那个）。
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
