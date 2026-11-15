//! 预约接口（列表 + 预约/取消预约）。列表与上新日历共用
//! `GET /reading/user/subscribe/list/v1/`（预约 tab_type=13）；
//! 路径常量与响应解析归 calendar 域。

use serde_json::Value;

use super::calendar::{CalendarPage, SUBSCRIBE_LIST_PATH, parse_calendar};
use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use hongguo_common::{AppError, AppResult};

/// 预约 / 取消预约端点：`POST`，body 为 **gzip 压缩的 JSON**
///（`op_type` 1 预约 / 2 取消），reading 轻签名、**必须带登录 cookie**；响应 `code==0` 即成功。
pub const SUBSCRIBE_OP_PATH: &str = "/reading/bookapi/search/uncover_subscribe/v";

/// 批量删除预约端点。
pub const SUBSCRIBE_DELETE_PATH: &str = "/reading/user/subscribe/delete/v";

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
        // 埋点上下文（官方原样字节对齐）
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

/// 批量删除预约：`POST /reading/user/subscribe/delete/v`；
/// `item_id` 即 subscribe 列表条目的 series_id（与 reserve_series 同源）。
pub async fn delete_reservations(
    item_ids: &[String],
    all_select: bool,
    not_del_item_ids: &[String],
    is_online: bool,
    env: &ApiEnv,
) -> AppResult<()> {
    if !all_select && item_ids.is_empty() {
        return Ok(());
    }
    let to_num = |s: &str| -> AppResult<i64> {
        s.parse()
            .map_err(|_| AppError::Media(format!("剧集 id 不是数字: {s}")))
    };
    let ids = item_ids
        .iter()
        .map(|s| to_num(s))
        .collect::<AppResult<Vec<i64>>>()?;
    let not_del = not_del_item_ids
        .iter()
        .map(|s| to_num(s))
        .collect::<AppResult<Vec<i64>>>()?;
    let payload = serde_json::json!({
        "item_id": ids,
        "all_select": all_select,
        "not_del_item_id": not_del,
        "is_online": is_online,
        "tab_type": 13,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造批量删预约请求失败: {e}")))?;
    let bytes = api_call_reading(LQ_API_ORIGIN, SUBSCRIBE_DELETE_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量删预约响应失败: {e}")))?;
    check_code(&value)
}
