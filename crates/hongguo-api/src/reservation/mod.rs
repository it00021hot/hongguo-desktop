//! 预约接口（列表 + 预约/取消预约）。列表与上新日历共用
//! `GET /reading/user/subscribe/list/v1/`（预约 tab_type=13）；
//! 路径常量与响应解析归 calendar 域。

use serde_json::Value;

use super::calendar::{CalendarPage, SUBSCRIBE_LIST_PATH, parse_calendar};
use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use hongguo_common::{AppError, AppResult, utils::json::check_code_rejected};

/// 预约 / 取消预约端点：`POST`，body 为 **gzip 压缩的 JSON**
///（`op_type` 1 预约 / 2 取消），reading 轻签名、**必须带登录 cookie**；响应 `code==0` 即成功。
pub const SUBSCRIBE_OP_PATH: &str = "/reading/bookapi/search/uncover_subscribe/v";
const SUBSCRIBE_DELETE_PATH: &str = "/reading/user/subscribe/delete/v";
const MY_SUBSCRIBED_CLIENT: i32 = 13;

/// 预约列表拉全（串行翻页，只给「需要全集做成员判定」的消费方用；预约页走 [`fetch_reservations_page`] 懒翻页）。
/// **session_id 绝不能自造常量**（服务端按它快照会话，操作后重读命中操作前缓存）；角标计数按拉全条数兜底（响应 total 部分形态缺失）。
pub async fn fetch_reservations(is_online: bool, env: &ApiEnv) -> AppResult<CalendarPage> {
    let mut merged = fetch_reservations_page(is_online, 0, None, env).await?;
    let server_total = if is_online {
        merged.online_total
    } else {
        merged.offline_total
    };
    // 翻页拉全；上限 20 页防空转，空页即止；续页 session_id 用首页响应下发的值
    let mut session_id = merged.session_id.clone();
    // 分页异常防线：offset 不前进即止；series_id 去重（重复页不撑大角标计数）
    let mut seen: std::collections::HashSet<String> =
        merged.items.iter().map(|i| i.series_id.clone()).collect();
    for _ in 0..19 {
        if !merged.has_more || merged.next_offset <= 0 {
            break;
        }
        let sid = Some(session_id.clone()).filter(|s| !s.is_empty());
        let next =
            fetch_reservations_page(is_online, merged.next_offset, sid.as_deref(), env).await?;
        if next.items.is_empty() || next.next_offset <= merged.next_offset {
            break;
        }
        if session_id.is_empty() {
            session_id = next.session_id.clone();
        }
        merged.items.extend(
            next.items
                .into_iter()
                .filter(|i| seen.insert(i.series_id.clone())),
        );
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

/// 拉一页预约列表（tab_type=13 形态，每页 20 条）——预约页入口，单页即回。
/// `session_id`：None = 首页（列表请求从不自带）；Some = 翻页续传首页响应下发的值，绝不能传自造常量。
pub async fn fetch_reservations_page(
    is_online: bool,
    offset: i64,
    session_id: Option<&str>,
    env: &ApiEnv,
) -> AppResult<CalendarPage> {
    let response = fetch_reservation_wire(is_online, offset, session_id, env).await?;
    check_code(&response)?;
    parse_calendar(response.get("data"))
}

async fn fetch_reservation_wire(
    is_online: bool,
    offset: i64,
    session_id: Option<&str>,
    env: &ApiEnv,
) -> AppResult<Value> {
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
    serde_json::from_slice(&bytes).map_err(|e| AppError::Rejected(format!("解析预约列表失败: {e}")))
}

/// Mutation verification must never interpret a partial or malformed result as absence.
pub async fn fetch_reservations_for_verification(
    online: bool,
    env: &ApiEnv,
) -> AppResult<Vec<super::calendar::CalendarItem>> {
    let total_field = if online {
        "online_total_count"
    } else {
        "offline_total_count"
    };
    let mut items = Vec::new();
    let mut expected_total = None;
    let mut seen = std::collections::HashSet::new();
    let mut offset = 0;
    let mut session = String::new();
    for _ in 0..20 {
        let response = fetch_reservation_wire(
            online,
            offset,
            (!session.is_empty()).then_some(session.as_str()),
            env,
        )
        .await?;
        check_code_rejected(&response, "读取预约列表")?;
        let data = response
            .get("data")
            .ok_or_else(|| verification_error(online, "missing data"))?;
        let total = data
            .get(total_field)
            .map(|total| {
                total
                    .as_i64()
                    .filter(|total| *total >= 0)
                    .ok_or_else(|| verification_error(online, "invalid total"))
            })
            .transpose()?;
        if expected_total.is_none() {
            expected_total = total;
        }
        if total.is_some() && total != expected_total {
            return Err(verification_error(
                online,
                "total changed during pagination",
            ));
        }
        let has_more = data.get("has_more").and_then(Value::as_bool);
        let raw_count = match data.get("subscribe_items") {
            Some(Value::Array(items)) => items.len(),
            None | Some(Value::Null) if total == Some(0) && has_more != Some(true) => 0,
            _ => return Err(verification_error(online, "missing subscribe_items")),
        };
        let page = parse_calendar(Some(data))?;
        if page.items.len() != raw_count {
            return Err(verification_error(online, "unparsed reservation rows"));
        }
        for item in page.items {
            if !seen.insert(
                item.reservation_item_id
                    .clone()
                    .unwrap_or_else(|| item.series_id.clone()),
            ) {
                return Err(verification_error(online, "duplicate reservation rows"));
            }
            items.push(item);
        }
        let has_more = has_more
            .or_else(|| {
                expected_total
                    .filter(|total| *total == items.len() as i64)
                    .map(|_| false)
            })
            .ok_or_else(|| verification_error(online, "indeterminate pagination"))?;
        if !has_more {
            if expected_total.is_some_and(|total| total != items.len() as i64) {
                return Err(verification_error(
                    online,
                    "reservation count does not match",
                ));
            }
            return Ok(items);
        }
        if page.next_offset <= offset || raw_count == 0 {
            return Err(verification_error(online, "pagination stopped advancing"));
        }
        if session.is_empty() {
            session = page.session_id;
        }
        offset = page.next_offset;
    }
    Err(verification_error(
        online,
        "maximum verification pages reached",
    ))
}

fn verification_error(online: bool, reason: &str) -> AppError {
    AppError::ReservationVerification(format!("online={online}: {reason}"))
}

/// 预约（reserve=true）或取消预约一部短剧。需要登录环境（env 带
/// sessionid cookie），匿名调用会被服务端静默拒绝。
pub async fn reserve_series(series_id: &str, reserve: bool, env: &ApiEnv) -> AppResult<()> {
    let item_id: i64 = series_id
        .parse()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| AppError::InvalidArgs("预约剧集标识无效".into()))?;
    let payload = serde_json::json!({
        "item_id": item_id,
        "item_type": 1,
        "op_type": if reserve { 1 } else { 2 },
        // 埋点上下文（官方原样字节对齐）
        "shark_param": {
            "enter_from": "BulletActivity",
            "page_list": "MainFragmentActivity,BulletActivity",
            "previous_page": "MainFragmentActivity",
        },
        "wish_list_all_del": 0,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Internal(format!("构造预约请求失败: {e}")))?;
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_OP_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Rejected(format!("解析预约响应失败: {e}")))?;
    check_code_rejected(&value, if reserve { "预约" } else { "取消预约" })
}

#[derive(serde::Serialize)]
struct DeleteSelection<'a> {
    item_id: &'a [String],
    not_del_item_id: &'a [String],
    all_select: bool,
    is_online: bool,
    tab_type: i32,
}

pub async fn delete_selected(item_ids: &[String], online: bool, env: &ApiEnv) -> AppResult<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    if item_ids.iter().any(|id| {
        id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || id.bytes().all(|b| b == b'0')
    }) {
        return Err(AppError::InvalidArgs("预约条目标识无效".into()));
    }
    let payload = DeleteSelection {
        item_id: item_ids,
        not_del_item_id: &[],
        all_select: false,
        is_online: online,
        tab_type: MY_SUBSCRIBED_CLIENT,
    };
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Internal(format!("构造批量取消预约请求失败: {e}")))?;
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_DELETE_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Rejected(format!("解析批量取消预约响应失败: {e}")))?;
    check_code_rejected(&value, "批量取消预约")
}

pub(crate) mod selectors;
mod upcoming;

pub use selectors::{SelectorId, SelectorOption, SelectorRow, SelectorSelection};

pub use upcoming::{
    UpcomingAudience, UpcomingFilters, UpcomingItem, UpcomingPage, fetch_upcoming,
    fetch_upcoming_panel,
};
