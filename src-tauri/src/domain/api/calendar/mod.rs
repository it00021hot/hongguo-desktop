//! 上新日历接口（subscribe/list 的日历形态：`tab_type=5` +
//! `need_calendar_schema=true`；与预约列表共用同一端点，响应解析也在本域，
//! 2026-10-03/04 抓包锁定，详见 docs/hongguo-api-endpoints.md）。

mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use crate::error::{AppError, AppResult};
use crate::utils::json::{check_code, int_field, num_field, str_field};
use crate::utils::time::beijing_date;

pub use model::{CalendarItem, CalendarPage};

/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// 上新日历（subscribe/list 的日历形态：tab_type=5 + need_calendar_schema）。
///
/// 2026-10-11 抓 hgplayer 1.1.8 实流对齐（captures/flows-calendar-align.jsonl），
/// 三处此前不知道的差异：
/// - **首页请求不带 `target_date`，但必须带 `need_personal_recommend=1`**——
///   缺了它服务端回的是「全部定档剧」混排（含大量已上线），条目集与第三方
///   完全对不上；带上后回「个性化预约推荐」混排（未上线 + 人预约数）；
/// - **翻页三键齐传 `offset` + `subscribe_offset`（同值）+ `session_id`（响应
///   回传）**——只传 offset 服务端无视，永远回同一页（死循环同页实测）；
/// - **响应是多天混排，按 `default_date`/选中日过滤后才是一天的条目**——
///   他家首页响应照样 has_more=true，滚到底第二页回来无新增才显示
///   「没有更多了」。
///
/// 切日期的真实参数是 **`target_date=YYYYMMDD`**（2026-10-04 抓 hgplayer
/// 切日期锁定；他家翻页时也始终在参）。响应条目有两种 schema（嵌套
/// `subscribe_data.*` / 扁平 `item_id/name...`），[`parse_calendar_item`] 都吃。
pub async fn fetch_new_calendar(date: Option<&str>, env: &ApiEnv) -> AppResult<CalendarPage> {
    let base: Vec<(String, String)> = [
        ("active_panel", "6"),
        ("gender_type", "2"),
        ("need_calendar_schema", "true"),
        ("need_personal_recommend", "1"),
        ("tab_style", "2"),
        ("tab_type", "5"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    // 首页（默认日）不带 target_date（hgplayer 首页实测无此键）；切日期才带
    let target = match date {
        Some(d) if !d.is_empty() => Some(d.to_string()),
        _ => None,
    };
    let mut q = base.clone();
    if let Some(d) = &target {
        q.push(("target_date".to_string(), d.clone()));
    }
    let first = fetch_calendar_page(&q, env).await?;
    let schema_dates = first.dates.clone();
    // 展示日：显式选择的日期，或首页 schema 的 default_date（混排按它过滤）
    let active_day = target.clone().unwrap_or_else(|| first.default_date.clone());
    let default_date = first.default_date.clone();

    // 翻页吃满（offset + subscribe_offset 同值 + session_id 回传；上限 16 页
    // 防服务端游标异常打穿）；翻页时 target_date 始终在参（hgplayer 同款）
    let mut all = first.items.clone();
    let mut page = first;
    for _ in 0..16 {
        if !page.has_more || page.next_offset <= 0 {
            break;
        }
        let mut q = base.clone();
        q.push(("target_date".to_string(), active_day.clone()));
        q.push(("offset".to_string(), page.next_offset.to_string()));
        q.push(("subscribe_offset".to_string(), page.next_offset.to_string()));
        if !page.session_id.is_empty() {
            q.push(("session_id".to_string(), page.session_id.clone()));
        }
        page = fetch_calendar_page(&q, env).await?;
        all.extend(page.items.clone());
    }

    // 混排按展示日过滤（他家 UI 同款口径）；服务端已按日返回时过滤是空操作
    let mut items: Vec<CalendarItem> = all
        .iter()
        .filter(|i| beijing_date(i.publish_time) == active_day)
        .cloned()
        .collect();
    // 过滤后为空但原始非空：beijing_date 解析不了 publish_time（未定档条目）
    // 会全被滤掉——保留原始顺序兜底，避免某天误显示为空
    if items.is_empty() && !all.is_empty() && target.is_none() {
        items = all;
    }
    let dates = if schema_dates.is_empty() {
        page.dates.clone()
    } else {
        schema_dates
    };
    Ok(CalendarPage {
        dates,
        default_date,
        items,
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
    let page = parse_calendar(value.get("data"))?;
    log::info!(
        "[Calendar] q={q:?} -> items={} has_more={} next={} first={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset,
        page.items.first().map(|i| (&i.title, i.is_online))
    );
    Ok(page)
}

/// 解析 subscribe/list 日历形态的 data 节点。
pub(super) fn parse_calendar(data: Option<&Value>) -> AppResult<CalendarPage> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    use crate::domain::api::rank::probe::anon_env;

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
