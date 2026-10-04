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

use super::client::{api_call_full, ApiEnv};
use super::discover::{check_code, int_field, num_field, str_field};
use crate::error::{AppError, AppResult};
use super::danmaku::LQ_API_ORIGIN;

/// 排行榜（书城 cell 换一换，/v 无斜杠）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";
/// 新剧推荐（v1/ 带斜杠——两条路径后缀不一致是服务端原状，抓包实锤）。
pub const NEW_DRAMA_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v1/";
/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// 排行榜页的 8 个榜单（顺序即 UI 竖排自上而下）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RankList {
    Recommend,
    HotPlay,
    Prestige,
    Subscribe,
    NewDrama,
    HotSearch,
    MustWatch,
    Followed,
}

impl RankList {
    /// 抓包得到的 `sub_selected_items` 标识。
    pub fn as_sub_selected(self) -> &'static str {
        match self {
            RankList::Recommend => "ranklist_hot_sc",
            RankList::HotPlay => "ranklist_hot_play_sc",
            RankList::Prestige => "ranklist_prestige",
            RankList::Subscribe => "ranklist_subscribe",
            RankList::NewDrama => "ranklist_new_rank_sc",
            RankList::HotSearch => "ranklist_hot_search_sc",
            RankList::MustWatch => "ranklist_must_watch",
            RankList::Followed => "ranklist_followed",
        }
    }

    /// 全部榜单（探测/测试遍历用；command 层按前端传的 kind 单独取）。
    #[cfg(test)]
    pub fn all() -> [RankList; 8] {
        [
            RankList::Recommend,
            RankList::HotPlay,
            RankList::Prestige,
            RankList::Subscribe,
            RankList::NewDrama,
            RankList::HotSearch,
            RankList::MustWatch,
            RankList::Followed,
        ]
    }
}

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

/// 一页榜单。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPage {
    pub items: Vec<RankItem>,
}

/// 拉一个榜单（全量一次返回，无翻页；抓包 offset/limit 固定 0）。
pub async fn fetch_rank(list: RankList, env: &ApiEnv) -> AppResult<RankPage> {
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
        ("sub_selected_items", list.as_sub_selected()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_full(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
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
    let bytes = api_call_full(LQ_API_ORIGIN, NEW_DRAMA_CELL_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析新剧失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
}

/// 预约列表（is_online=true 已上线 / false 待上线）。
pub async fn fetch_reservations(is_online: bool, env: &ApiEnv) -> AppResult<RankPage> {
    let q: Vec<(String, String)> = [
        ("is_online", if is_online { "true" } else { "false" }),
        ("limit", "20"),
        ("offset", "0"),
        ("subscribe_offset", "0"),
        ("subscribe_order_type", "0"),
        ("swipe_type", "0"),
        ("tab_type", "13"),
        ("need_calendar_schema", "false"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_full(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析预约失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
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
        default_date: page.default_date,
        items: picked,
        has_more: false,
        next_offset: 0,
    })
}

/// 拉一页日历（subscribe/list 日历形态）。
async fn fetch_calendar_page(q: &[(String, String)], env: &ApiEnv) -> AppResult<CalendarPage> {
    let bytes = api_call_full(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, q, env).await?;
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
    fn rank_list_covers_all_eight() {
        let subs: Vec<&str> = RankList::all().iter().map(|l| l.as_sub_selected()).collect();
        assert_eq!(subs.len(), 8);
        assert!(subs.contains(&"ranklist_hot_sc"));
        assert!(subs.contains(&"ranklist_must_watch"));
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

    /// 排行榜直连：8 个榜单逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for list in RankList::all() {
            match fetch_rank(list, &env).await {
                Ok(page) => {
                    let top = page.items.first();
                    println!(
                        "[rank/{:?}] {} 条, #1={:?} rank={:?} rec={:?}",
                        list,
                        page.items.len(),
                        top.map(|i| i.title.as_str()).unwrap_or(""),
                        top.map(|i| i.rank).unwrap_or(0),
                        top.map(|i| i.rec_text.as_str()).unwrap_or(""),
                    );
                    assert!(!page.items.is_empty(), "{list:?} 榜单不应为空");
                }
                Err(e) => panic!("[rank/{list:?}] ERR {e}"),
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

    /// 预约列表直连（匿名应返回空表 code==0）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reservations() {
        let env = anon_env();
        let page = fetch_reservations(true, &env).await.expect("预约列表");
        println!("[reservations] {} 条（匿名空表属正常）", page.items.len());
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
        let bytes = api_call_full(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, &env)
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
