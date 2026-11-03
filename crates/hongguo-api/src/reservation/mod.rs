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

#[cfg(test)]
mod tests {
    use super::super::calendar::CalendarItem;
    use super::*;

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
        assert_eq!(it.publish_time, 1_791_334_800_000);
        assert!(page.dates.is_empty(), "预约列表无日历 schema");
        assert_eq!(page.online_total, 0);
        assert_eq!(page.offline_total, 1, "待上线计数供 tab 徽标用");
    }

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

    #[test]
    fn reserve_payload_roundtrips_through_gzip() {
        let raw = br#"{"item_id":7692797468404091929,"item_type":1,"op_type":1}"#;
        let gz = crate::client::gzip_bytes(raw).expect("gzip");
        assert_ne!(gz, raw.to_vec(), "应真的压缩了");
        let mut back = flate2::read::GzDecoder::new(gz.as_slice());
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut back, &mut out).expect("解压");
        assert_eq!(out, raw);
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use crate::rank::probe::anon_env;

    /// 预约列表直连：匿名空表；HG_RESERVE_COOKIES 登录态验证两 tab 解析。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reservations() {
        let env = match std::env::var("HG_RESERVE_COOKIES") {
            Ok(c) if !c.is_empty() => {
                let device = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
                ApiEnv {
                    proxy: hongguo_common::model::ProxyConfig::default(),
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

    /// 预约/取消往返直连（HG_RESERVE_COOKIES 登录态，HG_RESERVE_SERIES 指定目标剧）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reserve_roundtrip() {
        let Ok(cookies) = std::env::var("HG_RESERVE_COOKIES") else {
            panic!("HG_RESERVE_COOKIES 必填（登录态 cookies，从抓包 cookie 头提取）");
        };
        let series =
            std::env::var("HG_RESERVE_SERIES").unwrap_or_else(|_| "7692797468404091929".into());
        let device = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
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
}
