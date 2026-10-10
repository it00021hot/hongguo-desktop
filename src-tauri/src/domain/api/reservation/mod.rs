//! 预约接口（列表 + 预约/取消预约；2026-10-04 抓 hgplayer 1.1.3 实操锁定）。
//!
//! 列表与上新日历共用 `GET /reading/user/subscribe/list/v1/`（预约
//! `tab_type=13` + `is_online`；路径常量与响应解析归 calendar 域）。

use serde_json::Value;

use super::calendar::{CalendarPage, SUBSCRIBE_LIST_PATH, parse_calendar};
use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::check_code;
use crate::error::{AppError, AppResult};

/// 预约 / 取消预约端点（2026-10-04 抓 hgplayer 1.1.3 实操锁定）。///
/// `POST`，body 为 **gzip 压缩的 JSON**（带 `Content-Encoding: gzip`）：
/// `{"item_id": <series_id>, "item_type": 1, "op_type": 1 预约 / 2 取消,
/// "shark_param": {埋点上下文}, "wish_list_all_del": 0}`；query 只放
/// 设备指纹，头是 reading 轻签名（无 gorgon/argus），**必须带登录
/// cookie**。响应 `code==0` 即成功。
pub const SUBSCRIBE_OP_PATH: &str = "/reading/bookapi/search/uncover_subscribe/v";

/// 预约列表（is_online=true 已上线 / false 待上线）。
///
/// 登录后响应条目是扁平形态（`item_id/name/has_subscribed/...`），
/// 匿名空表；与日历共用 [`super::calendar::parse_calendar_item`] 的双形态解析。
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
    // 服务端分页异常的防线：offset 不前进即止；series_id 去重
    // （重复页最多浪费一页请求，不会把角标计数撑大）
    let mut seen: std::collections::HashSet<String> =
        merged.items.iter().map(|i| i.series_id.clone()).collect();
    for _ in 0..19 {
        if !merged.has_more || merged.next_offset <= 0 {
            break;
        }
        let sid = Some(session_id.clone()).filter(|s| !s.is_empty());
        let next = fetch_reservations_page(is_online, merged.next_offset, sid.as_deref(), env).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::calendar::CalendarItem;

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
    use crate::domain::api::rank::probe::anon_env;

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
}
