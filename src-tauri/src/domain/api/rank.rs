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

use super::client::{api_call_reading, ApiEnv};
use super::discover::{check_code, int_field, num_field, str_field};
use crate::error::{AppError, AppResult};
use super::danmaku::LQ_API_ORIGIN;

/// 排行榜（书城 cell 换一换，/v 无斜杠）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";
/// 新剧推荐（v1/ 带斜杠——两条路径后缀不一致是服务端原状，抓包实锤）。
pub const NEW_DRAMA_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v1/";
/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// reading 系会话标识（subscribe/list 抓包恒带；格式 `YYYYMMDDHHMMSS` +
/// 20 位大写 hex，进程生命周期内一个——与真实客户端同款语义）。
fn reading_session_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        use std::fmt::Write as _;
        let mut tail = String::new();
        for _ in 0..10 {
            let _ = write!(tail, "{:02X}", rand::random::<u8>());
        }
        format!(
            "{}{}",
            chrono::Local::now().format("%Y%m%d%H%M%S"),
            tail
        )
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
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
        tabs: parse_cell_selector(value.get("data")),
    })
}

/// 展开响应 `data.cell_view.cell_selector` 的 tab → 子榜 → 面板选项表。
///
/// 抓包结构（2026-10-05 captures/rank-selector-schema.json）：
/// `outer_row.items[]`（内容 tab，selector_item_id）→ `sub_cell_selector.
/// outer_row.items[]`（子榜）→ `panel_selector.inner_rows[]`（行 row_name +
/// items[] 选项，selection_type=1 单选）。
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
                            // 登录态一级形态下 sub 层是筛选选项，「总榜」的
                            // id 为空但必须保留（前端靠它渲染回退选项）
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
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析新剧失败: {e}")))?;
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
pub async fn fetch_reservations(is_online: bool, env: &ApiEnv) -> AppResult<CalendarPage> {
    let q: Vec<(String, String)> = [
        ("is_online", if is_online { "true" } else { "false" }),
        ("limit", "20"),
        ("offset", "0"),
        ("subscribe_offset", "0"),
        ("subscribe_order_type", "0"),
        ("swipe_type", "0"),
        ("tab_type", "13"),
        ("need_calendar_schema", "false"),
        ("session_id", reading_session_id()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析预约失败: {e}")))?;
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
    if let Ok(page) = fetch_calendar_page(&q, env).await {
        if page
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
        next_offset: data
            .get("next_offset")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        online_total: data
            .get("online_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        offline_total: data
            .get("offline_total_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
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
        let Some(series_id) = raw.get("series_id").and_then(Value::as_str).filter(|s| !s.is_empty())
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

    /// 登录态一级形态（2026-10-05 app 实连发现）：outer_row 直接是榜单，
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
        assert!(tabs[0].subs.iter().all(|s| s.panel.is_empty()), "一级形态无 panel 层");
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
        let v: Value = serde_json::json!({ "a": { "video_data": [1] }, "b": [ { "video_data": [2] } ] });
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
                first.as_object().map(|m| m.keys().take(20).collect::<Vec<_>>())
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
                page.items.iter().map(|i| i.title.as_str()).take(3).collect::<Vec<_>>()
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
        let series = std::env::var("HG_RESERVE_SERIES")
            .unwrap_or_else(|_| "7692797468404091929".into());
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: Some(cookies),
        };

        reserve_series(&series, false, &env).await.expect("取消预约");
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
        let other = page
            .dates
            .last()
            .map(String::as_str)
            .expect("至少一个日期");
        let page2 = fetch_new_calendar(Some(other), &env).await.expect("指定日期日历");
        println!(
            "[calendar:{other}] items={} #1={:?} online_flags={:?}",
            page2.items.len(),
            page2.items.first().map(|i| &i.title),
            page2.items.iter().map(|i| i.is_online).collect::<Vec<_>>()
        );
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
