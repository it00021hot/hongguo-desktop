//! 排行榜 / 新剧 / 预约 / 上新日历接口（2026-10-03 mitmproxy 抓包锁定）。
//!
//! 抓包口径（详见 docs/hongguo-api-endpoints.md）：
//! - 排行榜 `GET /reading/bookapi/bookmall/cell/change/v`（**/v 无斜杠**），
//!   固定 `cell_id=7470092475068071998, tab_type=26, selected_items=all`，
//!   榜单用 `sub_selected_items` 切换；
//! - 新剧 `GET .../cell/change/v1/`（**v1/ 带斜杠**，与排行榜不同路径），
//!   `selected_items=firstonlinetime_new, cell_gender`；
//! - 两者响应都是 `data.cell_view.cell_data[].video_data[]`（每块一条，
//!   `recommend_info` 是二次序列化 JSON 字符串，内含排名 `rank`）；
//! - 预约与上新日历共用 `GET /reading/user/subscribe/list/v1/`：
//!   预约 `tab_type=13` + `is_online`，日历 `tab_type=5` + `need_calendar_schema=true`。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），设备注册实现前，
//! 探测用例走 hgplayer 抓包里的已注册设备档案。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, num_field, str_field};
use crate::error::{AppError, AppResult};

/// 排行榜（书城 cell 换一换，/v 无斜杠）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";
/// 新剧推荐（v1/ 带斜杠——两条路径后缀不一致是服务端原状，抓包实锤）。
pub const NEW_DRAMA_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v1/";
/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// reading 系会话标识（格式 `YYYYMMDDHHMMSS` + 20 位大写 hex，进程
/// 生命周期内一个）。**只供上新日历使用**——预约列表绝不能带自造的
/// session_id：服务端按它维护浏览会话快照，操作后的重读会命中操作前
/// 的缓存（2026-10-09 实车归因，见 fetch_reservations 文档；当日抓包
/// 实证 hgplayer 的列表请求从不带该参数）。
fn reading_session_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        use std::fmt::Write as _;
        let mut tail = String::new();
        for _ in 0..10 {
            let _ = write!(tail, "{:02X}", rand::random::<u8>());
        }
        format!("{}{}", chrono::Local::now().format("%Y%m%d%H%M%S"), tail)
    })
}

/// 预约 / 取消预约端点（2026-10-04 抓 hgplayer 1.1.3 实操锁定）。///
/// `POST`，body 为 **gzip 压缩的 JSON**（带 `Content-Encoding: gzip`）：
/// `{"item_id": <series_id>, "item_type": 1, "op_type": 1 预约 / 2 取消,
/// "shark_param": {埋点上下文}, "wish_list_all_del": 0}`；query 只放
/// 设备指纹，头是 reading 轻签名（无 gorgon/argus），**必须带登录
/// cookie**。响应 `code==0` 即成功。
pub const SUBSCRIBE_OP_PATH: &str = "/reading/bookapi/search/uncover_subscribe/v";

/// 排行榜页的 8 个「全部」tab 子榜（probe 遍历用；生产路径的选项表由
/// 响应 `cell_selector` schema 下发，前端直接用字符串 id）。
#[cfg(test)]
pub(crate) const ALL_TAB_SUBS: &[&str] = &[
    "ranklist_hot_sc",
    "ranklist_hot_play_sc",
    "ranklist_prestige",
    "ranklist_subscribe",
    "ranklist_new_rank_sc",
    "ranklist_hot_search_sc",
    "ranklist_must_watch",
    "ranklist_followed",
];

/// 榜单条目：比信息流卡片多排名与榜单文案。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankItem {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    /// 榜单名次（recommend_info.rank，从 1 起）
    pub rank: u32,
    /// "玄幻·全200集" 形态的副标题
    #[serde(default)]
    pub sub_title: String,
    /// 评分（"8.0"，容错字符串）
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 榜单热点文案（rec_text_item.RecommendText，如 "13707万最高热度"）
    #[serde(default)]
    pub rec_text: String,
    /// 次级信息（secondary_info_list[].content，如 "258.7万收藏"）
    #[serde(default)]
    pub secondary_infos: Vec<String>,
    /// 简介（video_desc）
    #[serde(default)]
    pub description: String,
    /// 题材标签（category_schema 二次解析）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 未上线（分集数为 0）。hgplayer 同款状态分支：未上线行显示预约
    /// 按钮、已上线行显示播放按钮——按剧状态分，不看榜单 id
    #[serde(default)]
    pub upcoming: bool,
    /// 当前账号已预约（榜单条目自带 online_subscribed；匿名恒 false）
    #[serde(default)]
    pub reserved: bool,
    /// 季徽（sub_title_list 里「第N季」形态条目；标题旁小徽，无则空）
    #[serde(default)]
    pub season: String,
}

/// 排行榜筛选面板的一个选项（id 为空 = 「总榜」，清除筛选）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPanelItem {
    pub id: String,
    pub name: String,
}

/// 筛选面板的一行（row_name：综合/时代背景/主题情节/角色设定…）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPanelRow {
    pub name: String,
    pub items: Vec<RankPanelItem>,
}

/// 内容 tab 下的一个子榜（自带筛选面板 schema）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankSubList {
    pub id: String,
    pub name: String,
    /// 子榜描述行（sub_title，如 "10月4日已更新·基于红果观看/互动以及
    /// 个人兴趣排序"；官方显示在子榜名旁）
    #[serde(default)]
    pub description: String,
    pub panel: Vec<RankPanelRow>,
}

/// 顶部内容 tab（全部/真人剧/漫剧/AI剧/演员/系列剧，2026-10-05 抓包 +
/// `cell_selector` schema 锁定）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankTab {
    pub id: String,
    pub name: String,
    pub subs: Vec<RankSubList>,
}

/// 一页榜单。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPage {
    pub items: Vec<RankItem>,
    /// 内容 tab / 子榜 / 筛选面板的完整选项表（响应 cell_selector 原样
    /// 展开，每次请求都随行下发；前端首次拿到后即可渲染整套筛选 UI）
    #[serde(default)]
    pub tabs: Vec<RankTab>,
}

/// 拉任意 tab × 子榜 × 筛选组合的榜单。
///
/// - `selected`：内容 tab 的 `selected_items`（all/human/comic_series_rank/
///   ai_playlet/series_album…）
/// - `sub`：子榜的 `sub_selected_items`（ranklist_hot_sc/human_hot_sc…）
/// - `panel`：筛选面板选中项 `panel_selected_items`（gender_female /
///   cate_308 / style_1685…；单值——hgplayer 抓包实测每次点击整组替换）
pub async fn fetch_rank_ex(
    selected: &str,
    sub: &str,
    panel: Option<&str>,
    env: &ApiEnv,
) -> AppResult<RankPage> {
    let mut q: Vec<(String, String)> = [
        ("cell_id", "7470092475068071998"),
        ("tab_type", "26"),
        ("selected_items", selected),
        ("category_id", "0"),
        ("cell_sub_id", "0"),
        ("client_req_type", "2"),
        ("client_template", "2"),
        ("gender", "2"),
        ("limit", "0"),
        ("offset", "0"),
        ("sub_selected_items", sub),
        // 面板 schema（unlimited_selector）随请求下发，抓包恒带 2
        ("unlimited_selector_change_type", "2"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(p) = panel.filter(|p| !p.is_empty()) {
        q.push(("panel_selected_items".to_string(), p.to_string()));
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
        tabs: parse_cell_selector(value.get("data")),
    })
}

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

/// 一个推荐 tab 的 cell 配置（cell/change 的两个定位 id，每 tab 不同）。
#[derive(Debug, Clone)]
pub struct RecommendTabConfig {
    pub cell_id: String,
    pub bookstore_id: String,
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

/// 展开响应 `data.cell_view.cell_selector` 的 tab → 子榜 → 面板选项表。
///
/// 抓包结构（2026-10-05 captures/rank-selector-schema.json）：
/// `outer_row.items[]`（内容 tab，selector_item_id）→ `sub_cell_selector.
/// outer_row.items[]`（子榜，sub_title 为描述行）→ `panel_selector.
/// inner_rows[]`（行 row_name + items[] 选项，selection_type=1 单选）。
///
/// 形态由**客户端版本号**决定（probe_rank_login_form 二分实证，与登录态
/// 无关）：73932（7.3.9，hgplayer 1.1.3 内置红果版本）下发两级全量结构；
/// 71332 老版本身份退化成扁平一级结构（无「全部」tab、面板分组丢失）。
/// 设备档案已在启动时对齐版本（[`crate::signer::device::align_app_version`]），
/// 一级形态的兼容归一保留在前端 normalizeTabs 里。
fn parse_cell_selector(data: Option<&Value>) -> Vec<RankTab> {
    let Some(tabs) = data
        .and_then(|d| d.get("cell_view"))
        .and_then(|cv| cv.get("cell_selector"))
        .and_then(|cs| cs.get("outer_row"))
        .and_then(|r| r.get("items"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    tabs.iter()
        .filter_map(|tab| {
            let id = str_field(tab, "selector_item_id");
            if id.is_empty() {
                return None;
            }
            let subs = tab
                .get("sub_cell_selector")
                .and_then(|s| s.get("outer_row"))
                .and_then(|r| r.get("items"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|sub| {
                            let sid = str_field(sub, "selector_item_id");
                            // 一级形态（老版本身份）下 sub 层是筛选选项，
                            // 「总榜」的 id 为空但必须保留（前端靠它渲染回退选项）
                            if sid.is_empty() && str_field(sub, "show_name") != "总榜" {
                                return None;
                            }
                            let panel = sub
                                .get("panel_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| {
                                    rows.iter()
                                        .filter_map(|row| {
                                            let items: Vec<RankPanelItem> = row
                                                .get("items")
                                                .and_then(Value::as_array)
                                                .map(|arr| {
                                                    arr.iter()
                                                        .map(|it| RankPanelItem {
                                                            id: str_field(it, "selector_item_id"),
                                                            name: str_field(it, "show_name"),
                                                        })
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            if items.is_empty() {
                                                return None;
                                            }
                                            Some(RankPanelRow {
                                                name: str_field(row, "row_name"),
                                                items,
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            Some(RankSubList {
                                id: sid,
                                name: str_field(sub, "show_name"),
                                description: str_field(sub, "sub_title"),
                                panel,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(RankTab {
                name: str_field(tab, "show_name"),
                id,
                subs,
            })
        })
        .collect()
}

/// 新剧推荐页（`cell_gender`：2=全部，其余见抓包；分页 offset/limit）。
pub async fn fetch_new_drama(gender: i64, offset: i64, env: &ApiEnv) -> AppResult<RankPage> {
    let q: Vec<(String, String)> = [
        ("cell_id", "7431550523368554558"),
        ("selected_items", "firstonlinetime_new"),
        ("cell_gender", gender.to_string().as_str()),
        ("change_type", "1"),
        ("client_req_type", "1"),
        ("limit", "18"),
        ("offset", offset.to_string().as_str()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_reading(LQ_API_ORIGIN, NEW_DRAMA_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析新剧失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
        tabs: parse_cell_selector(value.get("data")),
    })
}

/// 预约列表（is_online=true 已上线 / false 待上线）。
///
/// 登录后响应条目是扁平形态（`item_id/name/has_subscribed/...`），
/// 匿名空表；与日历共用 [`parse_calendar_item`] 的双形态解析。
///
/// **session_id 协议（2026-10-09 实车归因）**：列表接口绝不能自造
/// 进程级恒定 session_id——服务端按它维护浏览会话快照，操作
/// （预约/取消）之后用同一个 session_id 再读，拿到的永远是操作前
/// 的缓存列表，表现就是「取消预约无效」「新预约不出现」（探针实锤：
/// 同一时刻去掉 session_id 立即读到新状态；hgplayer 的列表请求从不
/// 带该参数，翻页时才续传响应下发的值）。所以首页不带，翻页续传
/// 首页响应下发的 session_id。
///
/// tab 角标计数不直接信响应的 `*_total_count`：该键在部分形态下缺失
/// （解析层缺省为 0），这里翻页拉全后用全量条数兜底——请求本身按
/// is_online 由服务端过滤，条数即该 tab 的真实总数。
pub async fn fetch_reservations(is_online: bool, env: &ApiEnv) -> AppResult<CalendarPage> {
    let mut merged = fetch_reservations_page(is_online, 0, None, env).await?;
    let server_total = if is_online {
        merged.online_total
    } else {
        merged.offline_total
    };
    // 翻页拉全；上限 20 页防服务端分页异常时失控，空页即止。
    // 续页 session_id 用首页响应下发的值（协议：响应下发、翻页续传）
    let mut session_id = merged.session_id.clone();
    for _ in 0..19 {
        if !merged.has_more || merged.next_offset <= 0 {
            break;
        }
        let sid = Some(session_id.clone()).filter(|s| !s.is_empty());
        let next = fetch_reservations_page(is_online, merged.next_offset, sid.as_deref(), env).await?;
        if next.items.is_empty() {
            break;
        }
        if session_id.is_empty() {
            session_id = next.session_id.clone();
        }
        merged.items.extend(next.items);
        merged.has_more = next.has_more;
        merged.next_offset = next.next_offset;
    }
    Ok(finalize_reservations(merged, is_online, server_total))
}

/// 拉全收尾：角标计数用服务端 total（>0 时），缺失（0）则按全量条数
/// 兜底；翻页在拉全后终结，has_more/next_offset 复位。
fn finalize_reservations(
    mut page: CalendarPage,
    is_online: bool,
    server_total: i64,
) -> CalendarPage {
    let count = if server_total > 0 {
        server_total
    } else {
        page.items.len() as i64
    };
    if is_online {
        page.online_total = count;
    } else {
        page.offline_total = count;
    }
    page.has_more = false;
    page.next_offset = 0;
    page
}

/// 拉一页预约列表（tab_type=13 形态，每页 20 条）。
///
/// `session_id`：None = 首页（对齐 hgplayer：列表请求从不自带）；
/// Some = 翻页续传首页响应下发的值。绝不能传进程级自造常量——服务端
/// 按它维护会话快照，操作后的重读会命中操作前的缓存（fetch_reservations
/// 文档有归因记录）。
async fn fetch_reservations_page(
    is_online: bool,
    offset: i64,
    session_id: Option<&str>,
    env: &ApiEnv,
) -> AppResult<CalendarPage> {
    let mut q: Vec<(String, String)> = vec![
        (
            "is_online".into(),
            if is_online { "true" } else { "false" }.into(),
        ),
        ("limit".into(), "20".into()),
        ("offset".into(), offset.to_string()),
        ("subscribe_offset".into(), "0".into()),
        ("subscribe_order_type".into(), "0".into()),
        ("swipe_type".into(), "0".into()),
        ("tab_type".into(), "13".into()),
        ("need_calendar_schema".into(), "false".into()),
    ];
    if let Some(sid) = session_id {
        q.push(("session_id".into(), sid.to_string()));
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析预约失败: {e}")))?;
    check_code(&value)?;
    parse_calendar(value.get("data"))
}

/// 预约（reserve=true）或取消预约一部短剧。需要登录环境（env 带
/// sessionid cookie），匿名调用会被服务端静默拒绝。
pub async fn reserve_series(series_id: &str, reserve: bool, env: &ApiEnv) -> AppResult<()> {
    let item_id: i64 = series_id
        .parse()
        .map_err(|_| AppError::Media(format!("剧集 id 不是数字: {series_id}")))?;
    let payload = serde_json::json!({
        "item_id": item_id,
        "item_type": 1,
        "op_type": if reserve { 1 } else { 2 },
        // 埋点上下文（1.1.3 抓包原样字节对齐）
        "shark_param": {
            "enter_from": "BulletActivity",
            "page_list": "MainFragmentActivity,BulletActivity",
            "previous_page": "MainFragmentActivity",
        },
        "wish_list_all_del": 0,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造预约请求失败: {e}")))?;
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_OP_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析预约响应失败: {e}")))?;
    check_code(&value)
}

/// 上新日历的一条剧集。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarItem {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 简介（video_desc）
    #[serde(default)]
    pub description: String,
    /// 主分类（category，如 "逆袭"）
    #[serde(default)]
    pub category: String,
    /// 热度文案（rec_tags[].content，如 "374万热度"）
    #[serde(default)]
    pub rec_tags: Vec<String>,
    /// 排期上线时间（unix 秒；0 = 未定档）
    #[serde(default)]
    pub publish_time: i64,
    /// 是否已上线
    #[serde(default)]
    pub is_online: bool,
    /// 当前账号是否已预约（预约列表扁平形态下发；日历形态恒 false）
    #[serde(default)]
    pub has_subscribed: bool,
}

/// 上新日历页。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarPage {
    pub items: Vec<CalendarItem>,
    /// 可选日期（"20261003" 形式，接口给前后各一周）
    pub dates: Vec<String>,
    /// 默认选中日期
    #[serde(default)]
    pub default_date: String,
    /// 还有下一页（一周的条目按排期升序分布在多页里）
    #[serde(default)]
    pub has_more: bool,
    /// 下一页 offset（0 = 没有更多）
    #[serde(default)]
    pub next_offset: i64,
    /// 预约列表（tab_type=13）的两个 tab 计数；日历形态恒 0
    #[serde(default)]
    pub online_total: i64,
    #[serde(default)]
    pub offline_total: i64,
    /// 服务端下发的浏览会话标识：翻页续传用（响应下发、翻页时带回；
    /// 首页请求绝不能自带，否则命中服务端会话缓存——见 fetch_reservations）
    #[serde(default)]
    pub session_id: String,
}

/// 上新日历（subscribe/list 的日历形态：tab_type=5 + need_calendar_schema）。
///
/// 切日期的真实参数是 **`target_date=YYYYMMDD`**（2026-10-04 抓 hgplayer
/// 切日期锁定：传它服务端只回该日条目）。此前实测 `date` 等 8 个候选名
/// 全部被忽略，靠翻页收集兜底——那条路保留为 `target_date` 失效时的回退。
///
/// 响应条目有两种 schema，由服务端按请求形态分发（带 `install_id` Cookie
/// 或 `target_date` 时是扁平形态）：嵌套 `subscribe_data.*` 与扁平
/// `item_id/name/...`，[`parse_calendar_item`] 两种都吃。
pub async fn fetch_new_calendar(date: Option<&str>, env: &ApiEnv) -> AppResult<CalendarPage> {
    let base: Vec<(String, String)> = [
        ("active_panel", "6"),
        ("gender_type", "2"),
        ("need_calendar_schema", "true"),
        ("tab_style", "2"),
        ("tab_type", "5"),
        ("session_id", reading_session_id()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let first = fetch_calendar_page(&base, env).await?;
    let Some(target) = date.filter(|d| !d.is_empty() && *d != first.default_date) else {
        return Ok(first);
    };

    // 主路径：target_date 直查。命中判据是首条目归属目标日——若服务端
    // 某天忽略该参数，会退回默认日数据，此时走翻页兜底。
    let mut q = base.clone();
    q.push(("target_date".to_string(), target.to_string()));
    if let Ok(page) = fetch_calendar_page(&q, env).await
        && page
            .items
            .first()
            .is_some_and(|i| beijing_date(i.publish_time) == target)
    {
        // 目标日整日无上新时条目为空：解析成功即视为命中空日，
        // 日期条兜底用首页的 schema（target 响应偶发缺 date_list）
        let dates = if page.dates.is_empty() {
            first.dates.clone()
        } else {
            page.dates
        };
        return Ok(CalendarPage {
            dates,
            default_date: first.default_date,
            ..page
        });
    }

    // 兜底：把目标日的条目从后续页里收集齐（越过目标日即停）
    let mut picked: Vec<CalendarItem> = first
        .items
        .iter()
        .filter(|i| beijing_date(i.publish_time) == target)
        .cloned()
        .collect();
    let default_date = first.default_date.clone();
    let mut page = first;
    for _ in 0..16 {
        if !page.has_more || page.next_offset <= 0 {
            break;
        }
        let passed = page
            .items
            .last()
            .is_some_and(|i| beijing_date(i.publish_time).as_str() > target);
        if passed {
            break;
        }
        let mut q = base.clone();
        q.push(("offset".to_string(), page.next_offset.to_string()));
        page = fetch_calendar_page(&q, env).await?;
        picked.extend(
            page.items
                .iter()
                .filter(|i| beijing_date(i.publish_time) == target)
                .cloned(),
        );
    }
    Ok(CalendarPage {
        dates: page.dates,
        default_date,
        items: picked,
        has_more: false,
        next_offset: 0,
        online_total: 0,
        offline_total: 0,
        session_id: String::new(),
    })
}

/// 拉一页日历（subscribe/list 日历形态）。
async fn fetch_calendar_page(q: &[(String, String)], env: &ApiEnv) -> AppResult<CalendarPage> {
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析上新日历失败: {e}")))?;
    check_code(&value)?;
    parse_calendar(value.get("data"))
}

/// unix 秒 → 北京时区（UTC+8）的 "20261004" 形式日期。
///
/// 日历的日期桶按北京时间的"当天"划分；时间戳 0（未定档）不归任何一天。
fn beijing_date(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    // Hinnant 的 civil_from_days：days 自 1970-01-01
    let z = (ts + 8 * 3600).div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// 解析 subscribe/list 日历形态的 data 节点。
fn parse_calendar(data: Option<&Value>) -> AppResult<CalendarPage> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;

    let mut items = Vec::new();
    for raw in data
        .get("subscribe_items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(item) = parse_calendar_item(raw) {
            items.push(item);
        }
    }

    let cal = data.get("calendar_schema");
    Ok(CalendarPage {
        items,
        dates: cal
            .and_then(|c| c.get("date_list"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        default_date: cal
            .and_then(|c| c.get("default_date"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        online_total: data
            .get("online_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        offline_total: data
            .get("offline_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        session_id: data
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 一条日历条目，两种 schema 都吃（2026-10-04 实测服务端按请求形态分发）：
///
/// - 嵌套形态（匿名请求）：`subscribe_data.{series_id,title,vid,...}` +
///   顶层 `category/rec_tags/schedule_publish_time/is_online`；
/// - 扁平形态（带 install_id Cookie 或 target_date）：字段直接在条目上
///   （`item_id/name/cover/item_desc/sub_title_list/...`），没有 vid——
///   前端点击走 seriesId 解析详情，vid 留空无害。
fn parse_calendar_item(raw: &Value) -> Option<CalendarItem> {
    if let Some(sd) = raw.get("subscribe_data") {
        let series_id = sd
            .get("series_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())?;
        return Some(CalendarItem {
            series_id: series_id.to_string(),
            title: str_field(sd, "title"),
            cover: str_field(sd, "cover"),
            vid: str_field(sd, "vid"),
            score: num_field(sd, "score"),
            play_cnt: int_field(sd, "play_cnt"),
            episode_cnt: int_field(sd, "episode_cnt").max(0) as u32,
            description: str_field(sd, "video_desc"),
            category: str_field(raw, "category"),
            rec_tags: string_array(raw, "rec_tags"),
            publish_time: int_field(raw, "schedule_publish_time"),
            is_online: raw
                .get("is_online")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_subscribed: raw
                .get("has_subscribed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let series_id = raw
        .get("item_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    Some(CalendarItem {
        series_id: series_id.to_string(),
        title: str_field(raw, "name"),
        cover: str_field(raw, "cover"),
        vid: String::new(),
        score: 0.0,
        play_cnt: 0,
        episode_cnt: 0,
        description: str_field(raw, "item_desc"),
        category: str_field(raw, "category"),
        rec_tags: string_array(raw, "sub_title_list"),
        publish_time: int_field(raw, "schedule_publish_time"),
        is_online: raw
            .get("is_online")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_subscribed: raw
            .get("has_subscribed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `[{content: "..."}, ...]` 形态的标签数组 → 字符串数组。
fn string_array(node: &Value, key: &str) -> Vec<String> {
    node.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| t.get("content").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 遍历 `data` 下所有 `video_data` 数组（cell_data 是嵌套结构，逐块平铺）。
fn collect_video_data(node: &Value, out: &mut Vec<Value>) {
    if let Some(arr) = node.as_array() {
        for item in arr {
            collect_video_data(item, out);
        }
        return;
    }
    let Some(obj) = node.as_object() else { return };
    if let Some(vd) = obj.get("video_data").and_then(Value::as_array) {
        out.extend(vd.iter().cloned());
    }
    for (_k, v) in obj {
        collect_video_data(v, out);
    }
}

/// cell_view 结构 → 平铺榜单条目。
fn parse_rank_items(data: Option<&Value>) -> AppResult<Vec<RankItem>> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut raws = Vec::new();
    collect_video_data(data, &mut raws);

    let mut items = Vec::new();
    for raw in &raws {
        let Some(series_id) = raw
            .get("series_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let rank = raw
            .get("recommend_info")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|info| int_field(&info, "rank").try_into().ok())
            .unwrap_or(0);
        let rec_text = raw
            .get("rec_text_item")
            .and_then(|t| t.get("RecommendText"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let secondary_infos = raw
            .get("secondary_info_list")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("content").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        items.push(RankItem {
            series_id: series_id.to_string(),
            title: str_field(raw, "title"),
            cover: str_field(raw, "cover"),
            vid: str_field(raw, "vid"),
            rank,
            sub_title: str_field(raw, "sub_title"),
            score: num_field(raw, "score"),
            play_cnt: int_field(raw, "play_cnt"),
            // 未上线判定：预约榜条目分集数为 0（已上线剧恒 >0；2026-10-09
            // 抓包预约榜条目 episode_cnt=0/play_cnt=0，已上线条目 132 集）
            upcoming: int_field(raw, "episode_cnt") == 0,
            reserved: raw
                .get("online_subscribed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            season: raw
                .get("sub_title_list")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .filter_map(|x| x.get("content").and_then(Value::as_str))
                        .find(|c| c.starts_with('第') && c.ends_with("季"))
                        .map(str::to_string)
                })
                .unwrap_or_default(),
            episode_cnt: int_field(raw, "episode_cnt").max(0) as u32,
            rec_text,
            secondary_infos,
            description: str_field(raw, "video_desc"),
            tags: super::discover::parse_tags(raw.get("category_schema")),
        });
    }
    // 排名字段缺失时按抓包顺序（接口本身就是榜单序）
    items.sort_by_key(|i| if i.rank == 0 { u32::MAX } else { i.rank });
    Ok(items)
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

    /// 抓包样本形状：cell_data 每块 1 条 + recommend_info 二次序列化。
    #[test]
    fn parses_rank_items_from_cell_view() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_id": 7470092475068071998_i64,
                "cell_data": [
                    { "video_data": [{
                        "series_id": "7681495327056071704",
                        "title": "万妖图录传第十二季",
                        "cover": "https://x.heic",
                        "vid": "7681497259883645977",
                        "sub_title": "玄幻·全200集",
                        "score": "8.0",
                        "play_cnt": 6197486,
                        "episode_cnt": 200,
                        "rec_text_item": { "RecommendText": "13707万最高热度" },
                        "secondary_info_list": [{ "content": "258.7万收藏" }],
                        "recommend_info": "{\"rank\":\"1\"}",
                        "video_desc": "以妖魔之血为墨",
                    }]},
                    { "video_data": [{
                        "series_id": "",
                        "title": "废条目"
                    }]}
                ]
            }
        });
        let items = parse_rank_items(Some(&data)).unwrap();
        assert_eq!(items.len(), 1, "空 series_id 要跳过");
        let it = &items[0];
        assert_eq!(it.rank, 1);
        assert_eq!(it.rec_text, "13707万最高热度");
        assert_eq!(it.secondary_infos, vec!["258.7万收藏"]);
        assert_eq!(it.score, 8.0);
        assert_eq!(it.sub_title, "玄幻·全200集");
    }

    /// 抓包样本（2026-10-05 cell_selector）：tab → 子榜 → 面板行三层展开。
    #[test]
    fn parses_cell_selector_schema() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_selector": {
                    "outer_row": {
                        "items": [
                            {
                                "show_name": "全部",
                                "selector_item_id": "all",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    {
                                        "show_name": "推荐榜",
                                        "selector_item_id": "ranklist_hot_sc",
                                        "sub_title": "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
                                        "panel_selector": { "inner_rows": [
                                            {
                                                "row_name": "综合",
                                                "selection_type": 1,
                                                "items": [
                                                    { "show_name": "总榜", "selector_item_id": "" },
                                                    { "show_name": "女频", "selector_item_id": "gender_female", "is_selected": false },
                                                    { "show_name": "男频", "selector_item_id": "gender_male" }
                                                ]
                                            },
                                            {
                                                "row_name": "时代背景",
                                                "items": [
                                                    { "show_name": "古装", "selector_item_id": "cate_308" },
                                                    { "show_name": "校园", "selector_item_id": "cate_4" }
                                                ]
                                            }
                                        ] }
                                    },
                                    { "show_name": "空面板子榜", "selector_item_id": "ranklist_subscribe" }
                                ] }}
                            },
                            {
                                "show_name": "漫剧",
                                "selector_item_id": "comic_series_rank",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    {
                                        "show_name": "推荐榜",
                                        "selector_item_id": "comic_series_hot_rank",
                                        "panel_selector": { "inner_rows": [
                                            { "row_name": "画风", "items": [
                                                { "show_name": "3d", "selector_item_id": "style_1685" }
                                            ] }
                                        ] }
                                    }
                                ] }}
                            },
                            { "show_name": "无 id 项", "selector_item_id": "" }
                        ]
                    }
                }
            }
        });
        let tabs = parse_cell_selector(Some(&data));
        assert_eq!(tabs.len(), 2, "空 id 的 tab 跳过");
        assert_eq!(tabs[0].id, "all");
        assert_eq!(tabs[0].name, "全部");
        assert_eq!(tabs[0].subs.len(), 2);
        let sub = &tabs[0].subs[0];
        assert_eq!(sub.id, "ranklist_hot_sc");
        assert_eq!(
            sub.description, "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
            "子榜描述行（sub_title）供官方同款排版用"
        );
        assert_eq!(sub.panel.len(), 2);
        assert_eq!(sub.panel[0].name, "综合");
        // 「总榜」id 为空 = 清除筛选，也要在场（前端靠它渲染回退选项）
        assert_eq!(sub.panel[0].items[0].name, "总榜");
        assert_eq!(sub.panel[0].items[0].id, "");
        assert_eq!(sub.panel[0].items[1].id, "gender_female");
        assert_eq!(sub.panel[1].items[0].id, "cate_308");
        assert!(tabs[0].subs[1].panel.is_empty(), "无面板的子榜收空表");
        assert_eq!(tabs[1].subs[0].panel[0].items[0].id, "style_1685");
        // 无 cell_selector 的响应（旧缓存/异常）不报错
        assert!(parse_cell_selector(Some(&serde_json::json!({}))).is_empty());
    }

    /// 一级形态（老版本身份的退化结构，2026-10-05 probe 二分实证：由
    /// version_code 决定、与登录态无关）：outer_row 直接是榜单，
    /// sub_cell_selector 是**筛选选项**（女频/男频/题材…，无 panel 层），
    /// 「总榜」选项 id 为空但要保留。
    #[test]
    fn parses_flat_login_selector_schema() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_selector": {
                    "outer_row": {
                        "items": [
                            {
                                "show_name": "推荐榜",
                                "selector_item_id": "ranklist_hot_sc",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    { "show_name": "总榜", "selector_item_id": "" },
                                    { "show_name": "女频", "selector_item_id": "gender_female" },
                                    { "show_name": "男频", "selector_item_id": "gender_male" },
                                    { "show_name": "古装", "selector_item_id": "cate_308" }
                                ] }}
                            },
                            {
                                "show_name": "热搜榜",
                                "selector_item_id": "ranklist_hot_search_sc",
                                "sub_cell_selector": null
                            }
                        ]
                    }
                }
            }
        });
        let tabs = parse_cell_selector(Some(&data));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].id, "ranklist_hot_sc");
        assert_eq!(tabs[0].name, "推荐榜");
        assert_eq!(tabs[0].subs.len(), 4, "空 id 的「总榜」要保留");
        assert_eq!(tabs[0].subs[0].id, "");
        assert_eq!(tabs[0].subs[0].name, "总榜");
        assert_eq!(tabs[0].subs[1].id, "gender_female");
        assert!(
            tabs[0].subs.iter().all(|s| s.panel.is_empty()),
            "一级形态无 panel 层"
        );
        assert!(tabs[1].subs.is_empty(), "无 sub_cell_selector 收空表");
    }

    #[test]
    fn rank_items_sort_by_rank_missing_last() {
        let data: Value = serde_json::json!({
            "cell_data": [
                { "video_data": [
                    { "series_id": "b", "recommend_info": "{\"rank\":\"2\"}" },
                    { "series_id": "a", "recommend_info": "{\"rank\":\"1\"}" },
                    { "series_id": "c" }
                ]}
            ]
        });
        let items = parse_rank_items(Some(&data)).unwrap();
        let ids: Vec<&str> = items.iter().map(|i| i.series_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "按名次排，缺名次殿后");
    }

    #[test]
    fn collect_walks_nested_cells() {
        let v: Value =
            serde_json::json!({ "a": { "video_data": [1] }, "b": [ { "video_data": [2] } ] });
        let mut out = Vec::new();
        collect_video_data(&v, &mut out);
        assert_eq!(out.len(), 2);
    }

    /// 抓包样本形状：subscribe_items + calendar_schema（日历形态）。
    #[test]
    fn parses_calendar_page() {
        let data: Value = serde_json::json!({
            "subscribe_items": [
                {
                    "is_online": true,
                    "category": "逆袭",
                    "schedule_publish_time": 1790957041,
                    "rec_tags": [{ "content": "374万热度" }],
                    "subscribe_data": {
                        "series_id": "7683352559066549310",
                        "title": "穿越古代搞军工",
                        "cover": "https://x.heic",
                        "vid": "7683354704205581337",
                        "score": "8.4",
                        "play_cnt": 970,
                        "episode_cnt": 92,
                        "video_desc": "绑定重工系统逆袭",
                    }
                },
                { "subscribe_data": { "series_id": "" } }
            ],
            "calendar_schema": {
                "date_list": ["20260926", "20261003", "20261010"],
                "default_date": "20261003",
            }
        });
        let page = parse_calendar(Some(&data)).unwrap();
        assert_eq!(page.items.len(), 1, "空 series_id 要跳过");
        let it = &page.items[0];
        assert_eq!(it.title, "穿越古代搞军工");
        assert_eq!(it.category, "逆袭");
        assert_eq!(it.rec_tags, vec!["374万热度"]);
        assert_eq!(it.publish_time, 1790957041);
        assert!(it.is_online);
        assert_eq!(it.score, 8.4);
        assert_eq!(page.dates, vec!["20260926", "20261003", "20261010"]);
        assert_eq!(page.default_date, "20261003");
    }

    #[test]
    fn missing_calendar_schema_degrades_to_empty_dates() {
        let data: Value = serde_json::json!({ "subscribe_items": [] });
        let page = parse_calendar(Some(&data)).unwrap();
        assert!(page.items.is_empty());
        assert!(page.dates.is_empty());
        assert_eq!(page.default_date, "");
    }

    /// 2026-10-04 抓包形状：带 install_id Cookie / target_date 时服务端
    /// 下发扁平条目（无 subscribe_data，无 vid）。
    #[test]
    fn parses_flat_calendar_items() {
        let data: Value = serde_json::json!({
            "subscribe_items": [
                {
                    "item_id": "7689087934082862104",
                    "name": "聚宝仙盆之杂灵根才是真BOSS第十三季",
                    "cover": "https://x.heic",
                    "category": "逆袭",
                    "categories": ["逆袭", "反转"],
                    "is_online": false,
                    "item_desc": "杂灵根弟子贺平生……",
                    "sub_title_list": [
                        { "content": "328.7万人预约" },
                        { "content": "逆袭" }
                    ],
                    "schedule_publish_time": 1791216360
                },
                { "item_id": "" }
            ],
            "calendar_schema": {
                "date_list": ["20260927", "20261011"],
                "default_date": "20261004",
            },
            "has_more": true,
            "next_offset": 24
        });
        let page = parse_calendar(Some(&data)).unwrap();
        assert_eq!(page.items.len(), 1, "空 item_id 要跳过");
        let it = &page.items[0];
        assert_eq!(it.series_id, "7689087934082862104");
        assert_eq!(it.title, "聚宝仙盆之杂灵根才是真BOSS第十三季");
        assert_eq!(it.description, "杂灵根弟子贺平生……");
        assert_eq!(it.rec_tags, vec!["328.7万人预约", "逆袭"]);
        assert_eq!(it.publish_time, 1791216360);
        assert!(!it.is_online);
        assert!(it.vid.is_empty(), "扁平形态没有 vid，留空而非报错");
        assert!(page.has_more);
        assert_eq!(page.next_offset, 24);
    }

    /// 预约列表（tab_type=13）登录态响应：扁平条目 + has_subscribed。
    #[test]
    fn parses_reservation_list_with_subscribed_flag() {
        let data: Value = serde_json::json!({
            "subscribe_items": [
                {
                    "item_id": "7692797468404091929",
                    "name": "道士下山：我在人间斩妖八百年第三季",
                    "cover": "https://x.heic",
                    "category": "奇幻",
                    "is_online": false,
                    "has_subscribed": true,
                    "item_desc": "斩妖除魔……",
                    "sub_title_list": [{ "content": "17.2万人预约" }],
                    "schedule_publish_time": 1791334800
                }
            ],
            "online_total_count": 0,
            "offline_total_count": 1
        });
        let page = parse_calendar(Some(&data)).unwrap();
        assert_eq!(page.items.len(), 1);
        let it = &page.items[0];
        assert_eq!(it.series_id, "7692797468404091929");
        assert!(it.has_subscribed, "登录态预约列表应带 has_subscribed");
        assert!(!it.is_online);
        assert_eq!(it.publish_time, 1791334800);
        assert!(page.dates.is_empty(), "预约列表无日历 schema");
        assert_eq!(page.online_total, 0);
        assert_eq!(page.offline_total, 1, "待上线计数供 tab 徽标用");
    }

    /// 服务端没回 `*_total_count`（该键部分形态缺失，解析层缺省 0）时，
    /// 拉全收尾用全量条数兜底——角标不再恒 0。
    #[test]
    fn finalize_reservations_falls_back_to_item_count() {
        let page = CalendarPage {
            items: vec![CalendarItem::default(); 35],
            online_total: 0,
            offline_total: 0,
            ..Default::default()
        };
        let online = finalize_reservations(page.clone(), true, 0);
        assert_eq!(online.online_total, 35, "缺 total 时用条数兜底");
        assert_eq!(online.offline_total, 0, "只写当前 tab 的计数");
        assert!(!online.has_more);
        assert_eq!(online.next_offset, 0);

        let offline = finalize_reservations(page, false, 88);
        assert_eq!(offline.offline_total, 88, "服务端 total 有效时优先");
        assert_eq!(offline.online_total, 0);
    }

    /// 预约操作的 body：gzip 可解回，JSON 字段与抓包形态一致。
    #[test]
    fn reserve_payload_roundtrips_through_gzip() {
        let raw = br#"{"item_id":7692797468404091929,"item_type":1,"op_type":1}"#;
        let gz = crate::domain::api::client::gzip_bytes(raw).expect("gzip");
        assert_ne!(gz, raw.to_vec(), "应真的压缩了");
        let mut back = flate2::read::GzDecoder::new(gz.as_slice());
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut back, &mut out).expect("解压");
        assert_eq!(out, raw);
        // 压缩流带 gzip 魔数 1f 8b（服务端按 Content-Encoding: gzip 解）
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(crate::domain::model::ProxyConfig::default())
    }

    /// hgplayer 抓包里的已注册设备（bookmall 系在静态旧设备上报 110，
    /// 设备注册实现前用它验证端点形状）。
    ///
    /// 2026-10-04 更新：旧档案（device_id=1694…）当日搜索开始回 0 字节——
    /// 服务端按 install_id 风控，旧 id 已失效；换当日 hgplayer 新注册的
    /// 设备（captures/flows-20261004.jsonl）后恢复。
    pub fn hg_env(base: &ApiEnv) -> ApiEnv {
        let pairs: Vec<(&str, &str)> = vec![
            ("ac", "wifi"),
            ("aid", "8662"),
            ("app_name", "novelread"),
            ("cdid", "babf8a0e-8586-40ca-bb75-20a1a698b43f"),
            ("channel", "xiaomi_8662_64"),
            ("device_brand", "xiaomi"),
            ("device_id", "2715158266032906"),
            ("device_platform", "android"),
            ("device_type", "23127PN0CC"),
            ("dpi", "460"),
            ("host_abi", "arm64-v8a"),
            ("iid", "2715158266282762"),
            ("language", "zh"),
            ("manifest_version_code", "73932"),
            ("os", "android"),
            ("os_api", "34"),
            ("os_version", "14"),
            ("resolution", "1200*2670"),
            ("ssmix", "a"),
            ("update_version_code", "73932"),
            ("version_code", "73932"),
            ("version_name", "7.3.9.32"),
        ];
        let mut v = serde_json::to_value(crate::signer::video_device()).unwrap();
        v["fields"] = serde_json::Value::Array(
            pairs
                .into_iter()
                .map(|(k, val)| serde_json::json!([k, val]))
                .collect(),
        );
        v["user_agent"] = serde_json::Value::String(
            "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)".into(),
        );
        ApiEnv {
            proxy: base.proxy.clone(),
            device: serde_json::from_value(v).unwrap(),
            cookie: Some(
                "store-region=cn-gd; store-region-src=did; install_id=2715158266282762; ttreq=1$5c6c7c7cd605c0a9f176a0533d9c64755b5df874".into(),
            ),
            x_tt_token: None,
        }
    }

    /// 排行榜筛选面板 schema 直连：dump 完整响应到 captures/，供提取
    /// `unlimited_selector`（panel_selected_items 的选项 id 表，抓包响应被
    /// addon 截断拿不全）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_schema() {
        let env = hg_env(&anon_env());
        let q: Vec<(String, String)> = [
            ("cell_id", "7470092475068071998"),
            ("tab_type", "26"),
            ("selected_items", "all"),
            ("category_id", "0"),
            ("cell_sub_id", "0"),
            ("client_req_type", "2"),
            ("client_template", "2"),
            ("gender", "2"),
            ("limit", "0"),
            ("offset", "0"),
            ("sub_selected_items", "ranklist_hot_sc"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("排行榜请求");
        let out = std::env::current_dir()
            .ok()
            .map(|d| d.join("../captures/rank-selector-schema.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("captures/rank-selector-schema.json"));
        std::fs::write(&out, &bytes).expect("写 captures/rank-selector-schema.json");
        println!("[rank-selector] {} bytes -> {}", bytes.len(), out.display());
    }

    /// 排行榜 cell_selector 形态二分：服务端是否按客户端版本号分发
    /// 两级（内容 tab × 子榜 × 分组面板）/ 一级（扁平榜单）结构。
    /// 用法：PROBE_DEVICE / PROBE_COOKIE 由真实库导出，
    /// `PROBE_VARIANT=asis|v73932 cargo test probe_rank_login_form -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").expect("PROBE_COOKIE");
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());

        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "v73932" {
            let bump = [
                ["version_code", "73932"],
                ["version_name", "7.3.9.32"],
                ["manifest_version_code", "73932"],
                ["update_version_code", "73932"],
            ];
            let fields = v["fields"].as_array_mut().expect("fields");
            for [k, val] in bump {
                for f in fields.iter_mut() {
                    if f[0].as_str() == Some(k) {
                        f[1] = serde_json::json!(val);
                    }
                }
            }
        }
        let device: crate::signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let cookie = format!(
            "{}; {}",
            crate::signer::device::anonymous_cookie(&device),
            account_cookie
        );
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: Some(cookie),
            x_tt_token: None,
        };

        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, &env)
            .await
            .expect("榜单请求");
        println!("[{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
        println!(
            "[{variant}] items={} #1={:?}",
            page.items.len(),
            page.items.first().map(|i| i.title.as_str())
        );
    }

    /// 排行榜 cell_selector 形态二分（补充）：老版本号 + 匿名（无会话），
    /// 区分「版本号」与「登录会话」哪个才是 schema 分发开关。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form_anon() {
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "v73932" {
            let fields = v["fields"].as_array_mut().expect("fields");
            for f in fields.iter_mut() {
                match f[0].as_str() {
                    Some("version_code" | "manifest_version_code" | "update_version_code") => {
                        f[1] = serde_json::json!("73932");
                    }
                    Some("version_name") => f[1] = serde_json::json!("7.3.9.32"),
                    _ => {}
                }
            }
        }
        let device: crate::signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, &env)
            .await
            .expect("榜单请求");
        println!("[anon/{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
    }

    /// 排行榜内容 tab「演员」形态验证（ranklist_celebrity，抓包没点到，
    /// 响应 cell_data 若不是剧集形态则前端跳过该 tab）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_celebrity() {
        let env = hg_env(&anon_env());
        let q: Vec<(String, String)> = [
            ("cell_id", "7470092475068071998"),
            ("tab_type", "26"),
            ("selected_items", "ranklist_celebrity"),
            ("category_id", "0"),
            ("cell_sub_id", "0"),
            ("client_req_type", "2"),
            ("client_template", "2"),
            ("gender", "2"),
            ("limit", "0"),
            ("offset", "0"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("演员榜请求");
        let v: Value = serde_json::from_slice(&bytes).expect("JSON");
        println!("[celebrity] code = {:?}", v.get("code"));
        let data = &v["data"];
        println!(
            "[celebrity] data keys = {:?}",
            data.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        let mut raws = Vec::new();
        collect_video_data(data, &mut raws);
        println!("[celebrity] video_data 条数 = {}", raws.len());
        if let Some(first) = raws.first() {
            println!(
                "[celebrity] 首条 keys = {:?}",
                first
                    .as_object()
                    .map(|m| m.keys().take(20).collect::<Vec<_>>())
            );
            println!(
                "[celebrity] title = {:?}, series_id = {:?}, sub_title = {:?}",
                first.get("title").and_then(Value::as_str),
                first.get("series_id").and_then(Value::as_str),
                first.get("sub_title").and_then(Value::as_str),
            );
        }
    }

    /// 筛选面板参数真连：同一子榜，总榜 vs 女频 vs 题材，条目应变化且带 tabs schema。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_panel_filter() {
        let env = hg_env(&anon_env());
        let base = fetch_rank_ex("all", "ranklist_hot_sc", None, &env)
            .await
            .expect("总榜");
        println!(
            "[panel:总榜] {} 条 #1={:?} tabs={}",
            base.items.len(),
            base.items.first().map(|i| i.title.as_str()),
            base.tabs.len()
        );
        assert!(!base.items.is_empty());
        assert_eq!(base.tabs[0].id, "all", "tabs schema 应随行下发");

        for p in ["gender_female", "gender_male", "cate_308"] {
            let page = fetch_rank_ex("all", "ranklist_hot_sc", Some(p), &env)
                .await
                .unwrap_or_else(|e| panic!("panel={p}: {e}"));
            println!(
                "[panel:{p}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "panel={p} 不应为空");
        }

        // 内容 tab 切换（真人剧 / 漫剧 / 系列剧，抓包样本 id）
        for (sel, sub) in [
            ("human", "human_hot_sc"),
            ("comic_series_rank", "comic_series_hot_rank"),
            ("series_album", "series_album_hot_sc"),
        ] {
            let page = fetch_rank_ex(sel, sub, None, &env)
                .await
                .unwrap_or_else(|e| panic!("{sel}/{sub}: {e}"));
            println!(
                "[tab:{sel}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "{sel}/{sub} 不应为空");
        }
    }

    /// app 生产同款设备（video_device + 匿名 cookie）下发的 selector schema：
    /// 对照 hgplayer 档案——排查两端 tabs 是否一致（灰度可能按设备分发）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_app_device() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, &env)
            .await
            .expect("app 同款设备榜单");
        println!(
            "[app-device:tabs] {:?}",
            page.tabs
                .iter()
                .map(|t| (t.id.as_str(), t.name.as_str(), t.subs.len()))
                .collect::<Vec<_>>()
        );
        println!(
            "[app-device:subs] {:?}",
            page.tabs[0]
                .subs
                .iter()
                .map(|s| (s.id.as_str(), s.name.as_str(), s.panel.len()))
                .collect::<Vec<_>>()
        );
    }

    /// 排行榜直连：8 个「全部」tab 子榜逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for sub in ALL_TAB_SUBS {
            match fetch_rank_ex("all", sub, None, &env).await {
                Ok(page) => {
                    let top = page.items.first();
                    println!(
                        "[rank/{sub}] {} 条, #1={:?} rank={:?} rec={:?}",
                        page.items.len(),
                        top.map(|i| i.title.as_str()).unwrap_or(""),
                        top.map(|i| i.rank).unwrap_or(0),
                        top.map(|i| i.rec_text.as_str()).unwrap_or(""),
                    );
                    assert!(!page.items.is_empty(), "{sub} 榜单不应为空");
                }
                Err(e) => panic!("[rank/{sub}] ERR {e}"),
            }
        }
    }

    /// 新剧推荐直连。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_new_drama() {
        let env = hg_env(&anon_env());
        let page = fetch_new_drama(2, 0, &env).await.expect("新剧推荐");
        println!(
            "[new-drama] {} 条, #1={:?}",
            page.items.len(),
            page.items.first().map(|i| &i.title)
        );
        assert!(!page.items.is_empty());
    }

    /// 预约列表直连。匿名返回空表；设 HG_RESERVE_COOKIES（hgplayer
    /// 登录 cookie）则验证两个 tab 的解析（已上线=嵌套、待上线=扁平，
    /// 2026-10-04 实测同接口按 tab 分发两种 schema）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reservations() {
        let env = match std::env::var("HG_RESERVE_COOKIES") {
            Ok(c) if !c.is_empty() => {
                let device = crate::signer::video_device();
                ApiEnv {
                    proxy: crate::domain::model::ProxyConfig::default(),
                    device,
                    cookie: Some(c),
                    x_tt_token: None,
                }
            }
            _ => anon_env(),
        };
        for tab in [true, false] {
            let page = fetch_reservations(tab, &env)
                .await
                .unwrap_or_else(|e| panic!("预约列表(is_online={tab}): {e}"));
            println!(
                "[reservations:online={tab}] {} 条: {:?}",
                page.items.len(),
                page.items
                    .iter()
                    .map(|i| i.title.as_str())
                    .take(3)
                    .collect::<Vec<_>>()
            );
            if matches!(std::env::var("HG_RESERVE_COOKIES"), Ok(ref c) if !c.is_empty()) {
                assert!(!page.items.is_empty(), "登录态 {tab} tab 不应为空");
                assert!(
                    page.items.iter().all(|i| i.has_subscribed),
                    "登录态列表应带 has_subscribed"
                );
            }
        }
    }

    /// 预约 / 取消预约往返直连（需要登录态）。
    ///
    /// HG_RESERVE_COOKIES 提供 hgplayer 的登录 cookie（从其
    /// `hgplayer.db` 的 account 表或抓包 cookie 头提取）；HG_RESERVE_SERIES
    /// 指定目标剧（缺省用 2026-10-04 抓包会话里预约的那部）。
    /// 流程：取消 → 列表应为空 → 重新预约 → 列表应含该剧。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reserve_roundtrip() {
        let Ok(cookies) = std::env::var("HG_RESERVE_COOKIES") else {
            panic!("HG_RESERVE_COOKIES 必填（hgplayer.db account 表的 cookies）");
        };
        let series =
            std::env::var("HG_RESERVE_SERIES").unwrap_or_else(|_| "7692797468404091929".into());
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: Some(cookies),
            x_tt_token: None,
        };

        reserve_series(&series, false, &env)
            .await
            .expect("取消预约");
        let page = fetch_reservations(false, &env).await.expect("取消后列表");
        let gone = page.items.iter().all(|i| i.series_id != series);
        println!(
            "[reserve] 取消后待上线 {} 条, 目标剧已移除: {gone}",
            page.items.len()
        );
        assert!(
            gone,
            "取消后不应仍在列表: {:?}",
            page.items.iter().map(|i| &i.series_id).collect::<Vec<_>>()
        );

        reserve_series(&series, true, &env).await.expect("重新预约");
        let page2 = fetch_reservations(false, &env).await.expect("预约后列表");
        let it = page2.items.iter().find(|i| i.series_id == series);
        println!(
            "[reserve] 预约后待上线 {} 条, 目标剧: {:?}",
            page2.items.len(),
            it.map(|i| (&i.title, i.has_subscribed, i.is_online))
        );
        assert!(it.is_some(), "预约后应回到列表");
        assert!(it.unwrap().has_subscribed, "列表应标记 has_subscribed");
    }

    /// 上新日历直连：应返回日期列表 + 当日条目。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_new_calendar() {
        let env = anon_env();
        let page = fetch_new_calendar(None, &env).await.expect("上新日历");
        println!(
            "[calendar] dates={:?} default={:?} items={} #1={:?}",
            page.dates.first().zip(page.dates.last()),
            page.default_date,
            page.items.len(),
            page.items.first().map(|i| &i.title)
        );
        assert!(!page.dates.is_empty(), "日历应带日期列表");
        assert!(!page.items.is_empty(), "当日应有上新条目");
        // 对照：显式传一个非默认日期——修复后应翻页收集到该日条目
        let other = page.dates.last().map(String::as_str).expect("至少一个日期");
        let page2 = fetch_new_calendar(Some(other), &env)
            .await
            .expect("指定日期日历");
        println!(
            "[calendar:{other}] items={} #1={:?} online_flags={:?}",
            page2.items.len(),
            page2.items.first().map(|i| &i.title),
            page2.items.iter().map(|i| i.is_online).collect::<Vec<_>>()
        );
    }

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

    /// 日历 `target_date` 直查：生产同款环境（静态档案 + 匿名 Cookie）下
    /// 看响应形态与条目归属（2026-10-04 抓包锁定参数名后的转正实验）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_calendar_target_date() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let q: Vec<(String, String)> = [
            ("active_panel", "6"),
            ("gender_type", "2"),
            ("need_calendar_schema", "true"),
            ("tab_style", "2"),
            ("tab_type", "5"),
            ("target_date", "20261006"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, &env)
            .await
            .expect("target_date 请求");
        let v: Value = serde_json::from_slice(&bytes).expect("JSON");
        let data = &v["data"];
        println!(
            "[calendar:target] data 键 = {:?}",
            data.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        let items = data["subscribe_items"].as_array();
        println!("[calendar:target] 条目数 = {:?}", items.map(Vec::len));
        if let Some(first) = items.and_then(|arr| arr.first()) {
            println!(
                "[calendar:target] 首条目键 = {:?}",
                first.as_object().map(|m| m.keys().collect::<Vec<_>>())
            );
            println!(
                "[calendar:target] 有 subscribe_data = {}",
                first.get("subscribe_data").is_some()
            );
        }
        match parse_calendar(Some(data)) {
            Ok(page) => {
                let days: Vec<String> = page
                    .items
                    .iter()
                    .map(|i| beijing_date(i.publish_time))
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                println!(
                    "[calendar:target] 解析 {} 条, 日期桶 = {days:?}, #1 = {:?}",
                    page.items.len(),
                    page.items.first().map(|i| &i.title)
                );
            }
            Err(e) => println!("[calendar:target] 解析失败: {e}"),
        }
    }
}
