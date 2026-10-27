//! 弹幕：commentapi 的 comment/list（comment_type=20）。
//!
//! 协议口径：
//! - `POST {LQ_API_ORIGIN}/novel/commentapi/comment/list/{group_id}/v1/`，
//!   **group_id（=分集 vid）在路径里**；
//! - 参数在 body **顶层**（不包 biz_param），见 [`danmaku_payload`]；
//! - 自定义头 `x-reading-request: {ticket_ms}-{random}`；
//! - **滚窗协议**：不是一次拉全。起播从当前播放位置（`business_param.
//!   start_offset_time`）拉一窗（count=90），响应 cursor 原样回传 +
//!   `start_offset_time` 推进到其 `end_offset_time` 拉下一窗（30 秒窗），
//!   has_more=false 收尾；窗内还有分页，同样走 cursor；
//! - 弹幕的**时间轴**在 `comment.expand.offset_time`（毫秒，与 cursor 同单位）。
//! - ⚠️ 设备风控分（x-medusa/x-helios，签名占位实现）低于门槛时服务端
//!   把弹幕降级为每窗少量条目即 has_more=false——逐窗拉取仍优于一次拉全。
//!
//! 发送（comment/add）需要登录态，登录里程碑落地后再接。
//!
//! 评论区 / 剧评 / 回复链（同一端点的其余形态与 reply/list）在 [`comments`]，
//! 经本模块 re-export 对外。

mod comments;
mod model;

pub use comments::{
    fetch_comment_replies, fetch_comments_page, fetch_creator_profile, fetch_creator_works,
    fetch_review_replies, fetch_series_comments_page, send_series_review,
};
pub(crate) use comments::{parse_comment_item, parse_reply_item};
pub use model::{
    CommentItem, CommentPage, CommentTagStat, CreatorProfile, CreatorWork, CreatorWorksPage,
    Danmaku, ReplyItem, ReplyPage, SeriesReviewPage,
};

#[cfg(test)]
use comments::{
    comment_replies_payload, comments_payload, parse_reply_page, review_replies_payload,
    series_comments_payload,
};

use serde::Serialize;
use serde_json::Value;

use super::client::ApiEnv;
#[cfg(test)]
use crate::params::interaction::{
    COMMENT_TYPE_COMMENT, COMMENT_TYPE_REVIEW, GROUP_TYPE_REVIEW, LIST_SOURCE_REVIEW,
    LIST_SOURCE_REVIEW_REPLIES, SERVER_CHANNEL_COMMENT, SERVER_CHANNEL_REVIEW,
};
use crate::params::interaction::{
    COMMENT_TYPE_DANMAKU, GROUP_TYPE_COMMENT, LIST_SOURCE_DANMAKU, SERVER_CHANNEL_DANMAKU,
};
use hongguo_common::{AppError, AppResult};

/// reading 系 API 主机（取流 multi_video_model/player/v1
/// 也在这台机上，见 [`crate::client::LQ_API_ORIGIN`]；commentapi 只在
/// 这个主机上存在）。
pub const LQ_API_ORIGIN: &str = crate::client::LQ_API_ORIGIN;

/// 弹幕窗请求的 `need_danmaku_guide_type`（官方常态值）。
const DANMAKU_GUIDE_TYPE: i64 = 2;

/// 弹幕必须**随播放进度滚窗**（官方唯一形态）：medusa 风控
/// 降级下每窗少量条目且 `has_more=false`，不存在「一次拉全」路线——
/// 官方客户端就是播到哪拉到哪（窗序列 0/30s/60s/90s
/// 随播放推进）。壳侧按 [`fetch_danmaku_window_page`] + 播放位置推进
/// 实现（GPUI `advance_danmaku_window` / Tauri 播放页滚窗 effect）。
/// 注意 `business_param.playlet_item_duration` 仍须传真值毫秒（服务端
/// 端点校验需要，注释见命令侧）。
/// 一窗弹幕。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DanmakuWindowPage {
    pub items: Vec<Danmaku>,
    /// 还有下一窗/下一页。
    pub has_more: bool,
    /// 下一窗请求**原样回传**的 cursor（官方协议：cursor 由服务端下发，
    /// 客户端不改写——里面带着 danmaku_window_count 等服务端状态）。
    pub next_cursor: String,
    /// 下一窗的播放位置起点（ms）＝本窗 cursor 的 `end_offset_time`：
    /// 请求侧 `business_param.start_offset_time`
    /// 按它推进（cursor [0,30000) → 下一请求 start=30000）。
    pub next_start_ms: u64,
}

/// 请求体（business_param
/// 只有官方这四键）。
fn danmaku_payload(
    group_id: &str,
    book_id: &str,
    start_offset_ms: u64,
    item_duration_ms: u64,
    cursor: &str,
) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_danmaku_guide_type": [DANMAKU_GUIDE_TYPE],
            "playlet_item_duration": item_duration_ms,
            "start_offset_time": start_offset_ms,
        },
        "comment_source": LIST_SOURCE_DANMAKU,
        "comment_type": COMMENT_TYPE_DANMAKU,
        "compliance_status": 0,
        "count": 90,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "server_channel": SERVER_CHANNEL_DANMAKU,
        "sort": 1,
    })
}

/// 拉一窗弹幕（官方滚窗协议）。
///
/// 官方不是一次拉全：起播从当前播放位置拉一窗（count=90），之后在窗口
/// 边界回传上一响应的 cursor、`start_offset_time` 推进到其 `end_offset_time`
/// 拉下一窗；窗内还有分页（has_more=true 继续）。`item_duration_ms` 是
/// 本集真实时长（官方传 video_model 的 video_duration；解析不出传 0 也能拉）。
pub async fn fetch_danmaku_window_page(
    group_id: &str,
    book_id: &str,
    start_offset_ms: u64,
    item_duration_ms: u64,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<DanmakuWindowPage> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&danmaku_payload(
        group_id,
        book_id,
        start_offset_ms,
        item_duration_ms,
        cursor,
    ))
    .map_err(|e| AppError::Signer(e.to_string()))?;
    // reading 系统一入口：轻签名头 + gzip body
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析弹幕响应失败: {e}")))?;
    parse_window_page(&v)
}

/// 弹幕响应 → 一窗（纯函数，单测锚点）。
fn parse_window_page(v: &Value) -> AppResult<DanmakuWindowPage> {
    if v.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!(
            "弹幕接口返回 {}: {msg}",
            v.get("code").and_then(Value::as_i64).unwrap_or(-1)
        )));
    }
    let list_info = v
        .pointer("/data/common_list_info")
        .cloned()
        .unwrap_or(Value::Null);
    let mut items = Vec::new();
    for entry in v
        .pointer("/data/data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(comment) = entry.get("comment") else {
            continue;
        };
        let comment_id = comment
            .get("comment_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = comment
            .pointer("/common/content/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if comment_id.is_empty() || text.is_empty() {
            continue;
        }
        items.push(Danmaku {
            comment_id,
            text,
            offset_ms: comment
                .pointer("/expand/offset_time")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as u64,
            digg_count: comment
                .pointer("/stat/digg_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        });
    }
    let next_cursor = list_info
        .get("cursor")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let next_start_ms = serde_json::from_str::<Value>(&next_cursor)
        .ok()
        .and_then(|c| c.get("end_offset_time").and_then(Value::as_u64))
        .unwrap_or(0);
    Ok(DanmakuWindowPage {
        has_more: list_info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor,
        next_start_ms,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_shape_matches_capture() {
        // business_param 只有官方四键，
        // start_offset_time 是当前播放位置、playlet_item_duration 是本集真实时长
        let p = danmaku_payload(
            "7680669690372312089",
            "7680564528135818302",
            1959,
            199000,
            "",
        );
        // 锚点字段：任何一个改形都会 100103
        assert_eq!(p["comment_type"], COMMENT_TYPE_DANMAKU, "20 = 弹幕");
        assert_eq!(p["comment_source"], LIST_SOURCE_DANMAKU, "601");
        assert_eq!(p["server_channel"], SERVER_CHANNEL_DANMAKU, "1000");
        assert_eq!(p["group_type"], GROUP_TYPE_COMMENT);
        assert_eq!(p["count"], 90);
        assert_eq!(p["aid"], 8662);
        assert_eq!(p["group_id"], "7680669690372312089");
        assert_eq!(p["business_param"]["book_id"], "7680564528135818302");
        assert_eq!(p["business_param"]["start_offset_time"], 1959);
        assert_eq!(p["business_param"]["playlet_item_duration"], 199000);
        assert_eq!(
            p["business_param"]["need_danmaku_guide_type"],
            serde_json::json!([DANMAKU_GUIDE_TYPE])
        );
        assert_eq!(p["cursor"], "");
    }

    #[test]
    fn window_cursor_passthrough() {
        let p = danmaku_payload(
            "g",
            "b",
            30000,
            199000,
            r#"{"start_offset_time":0,"end_offset_time":30000}"#,
        );
        assert!(p["cursor"].as_str().unwrap().contains("end_offset_time"));
        assert_eq!(p["business_param"]["start_offset_time"], 30000);
    }

    #[test]
    fn window_next_start_parses_cursor_end() {
        // 响应 cursor 的 end_offset_time 是下一窗请求的 start_offset_time
        let v = serde_json::json!({
            "code": 0,
            "data": {
                "common_list_info": {
                    "has_more": true,
                    "cursor": r#"{"start_offset_time":0,"end_offset_time":30000,"danmaku_window_count":{"0":267}}"#,
                },
                "data_list": [],
            },
        });
        let page = parse_window_page(&v).unwrap();
        assert!(page.has_more);
        assert_eq!(page.next_start_ms, 30000);
        assert!(page.next_cursor.contains("danmaku_window_count"));
    }

    #[test]
    fn comments_payload_matches_capture_20261016() {
        // 单集维度 group=vid / group_type=30 / channel=18，count=10，
        // business_param.sort="video_innerflow"，need_count 仅首屏 true；
        // 弹幕形 business_param / channel=1000 会被 103001 拒
        let p = comments_payload("g", "b", "");
        assert_eq!(p["comment_type"], COMMENT_TYPE_COMMENT);
        assert_eq!(p["comment_source"], 4);
        assert_eq!(
            p["server_channel"], SERVER_CHANNEL_COMMENT,
            "评论走 18，弹幕才是 1000"
        );
        assert_eq!(p["group_type"], GROUP_TYPE_COMMENT);
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["business_param"]["req_type"], 0);
        assert_eq!(p["business_param"]["sort"], "video_innerflow");
        assert_eq!(p["business_param"]["start_offset_time"], 0);
        assert_eq!(p["count"], 10);
        assert!(p.get("cursor").is_none(), "首屏不带 cursor 键，翻页才回传");
        let p2 = comments_payload("g", "b", r#"{"session_id":"s","offset":10}"#);
        assert_eq!(
            p2["business_param"]["need_count"], false,
            "翻页 need_count=false"
        );
        assert_eq!(
            p2["cursor"], r#"{"session_id":"s","offset":10}"#,
            "翻页回传会话游标原样串"
        );
    }

    #[test]
    fn series_comments_payload_matches_capture_20261007() {
        // 剧级评论换形态——group_id 是
        // series_id、group_type=1、source=1/type=2/channel=34，
        // business_param 是全量字段组
        let p = series_comments_payload("7687941387362257982", "");
        assert_eq!(
            p["group_type"], GROUP_TYPE_REVIEW,
            "1 = 书/剧维度，30 是单集"
        );
        assert_eq!(p["comment_source"], LIST_SOURCE_REVIEW);
        assert_eq!(p["comment_type"], COMMENT_TYPE_REVIEW);
        assert_eq!(p["server_channel"], SERVER_CHANNEL_REVIEW);
        assert_eq!(p["group_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["book_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["count"], 10);
    }

    #[test]
    fn comment_replies_payload_matches_capture_20261010() {
        // 展开评论回复形态：source=504（评论拉取
        // 是 4）+ 首页显式空 cursor + compliance_status 在场——三处都与
        // comment/list 形态不同，混用会被拒
        let p = comment_replies_payload("g", "b", "c", "");
        assert_eq!(p["comment_source"], 504, "回复拉取走 504");
        assert_eq!(p["comment_type"], COMMENT_TYPE_COMMENT);
        assert_eq!(p["server_channel"], SERVER_CHANNEL_COMMENT);
        assert_eq!(p["group_type"], GROUP_TYPE_COMMENT);
        assert_eq!(p["group_id"], "g");
        assert_eq!(p["comment_id"], "c");
        assert_eq!(p["business_param"]["book_id"], "b");
        assert_eq!(p["cursor"], "", "评论维度首页是显式空串");
        assert_eq!(p["compliance_status"], 0);
        assert_eq!(p["count"], 10);
        // 翻页：cursor 原样回传数字串
        let p2 = comment_replies_payload("g", "b", "c", "10");
        assert_eq!(p2["cursor"], "10");
    }

    #[test]
    fn review_replies_payload_matches_capture_20261010() {
        // 展开/翻页剧评回复形态：
        // source=501/type=2/ch=34 + real_level=2 + 首屏无 cursor 键 +
        // need_count 首页 true 翻页 false
        let p = review_replies_payload("s", "c", "");
        assert_eq!(
            p["comment_source"], LIST_SOURCE_REVIEW_REPLIES,
            "剧评回复拉取走 501"
        );
        assert_eq!(p["comment_type"], COMMENT_TYPE_REVIEW);
        assert_eq!(p["server_channel"], SERVER_CHANNEL_REVIEW);
        assert_eq!(p["group_type"], GROUP_TYPE_REVIEW);
        assert_eq!(p["group_id"], "s");
        assert_eq!(p["comment_id"], "c");
        assert_eq!(p["business_param"]["real_level"], 2);
        assert_eq!(
            p["business_param"]["need_count"], true,
            "首页 need_count=true"
        );
        assert!(
            p.get("cursor").is_none(),
            "剧评维度首屏不带 cursor 键（评论维度才是显式空串）"
        );
        let p2 = review_replies_payload("s", "c", "20");
        assert_eq!(p2["cursor"], "20");
        assert_eq!(
            p2["business_param"]["need_count"], false,
            "翻页 need_count=false"
        );
    }

    #[test]
    fn reply_page_parses_capital_common() {
        // 回复条目在键名大写的 Common 下（上游序列化怪癖）；
        // 分页在 comment_list_info（不是 common_list_info）
        let v = serde_json::json!({
            "code": 0,
            "data": {
                "comment_list_info": {"cursor": "10", "has_more": true, "total": 29},
                "reply_list": [{
                    "Common": {
                        "content": {"text": "好[送花]"},
                        "create_timestamp": 1791575272,
                        "user_info": {
                            "user_id": "1968541249580363",
                            "base_info": {
                                "user_name": "路人甲",
                                "expand_user_avatar": "https://x/a.webp",
                            },
                        },
                    },
                    "expand": {},
                    "reply_id": "r1",
                    "reply_to_comment_id": "c1",
                    "reply_to_reply_id": "",
                    "reply_to_user_info": {"base_info": {"user_name": "楼主"}},
                    "stat": {"digg_count": 3, "reply_count": 0},
                    "user_action": {"user_digg": true},
                }],
            },
        });
        let page = parse_reply_page(&v).unwrap();
        assert_eq!(page.total, 29);
        assert!(page.has_more);
        assert_eq!(page.next_cursor, "10");
        let r = &page.items[0];
        assert_eq!(r.reply_id, "r1");
        assert_eq!(r.user_id, "1968541249580363");
        assert_eq!(r.text, "好[送花]");
        assert_eq!(r.user_name, "路人甲");
        assert_eq!(r.digg_count, 3);
        assert!(r.user_digg);
        assert_eq!(r.reply_to_name, "楼主");
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// probe 专用的翻页上限（评论区全量对账用；生产代码按页返回）。
    const MAX_WINDOWS: usize = 240;

    /// 拉一集的**全部**评论区（按 cursor 翻页到 has_more=false，上限见
    /// [`MAX_WINDOWS`]）。
    ///
    /// 热门集会拉几百页，command 层已改用 [`fetch_comments_page`] 按页返回；
    /// 本函数只服务 probe 全量对账，与生产代码无关。
    async fn fetch_comments_all(
        group_id: &str,
        book_id: &str,
        env: &ApiEnv,
    ) -> AppResult<CommentPage> {
        let mut all = CommentPage::default();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = String::new();
        for _ in 0..MAX_WINDOWS {
            let page = fetch_comments_page(group_id, book_id, &cursor, env).await?;
            if all.total == 0 {
                all.total = page.total;
            }
            all.has_more = page.has_more;
            all.next_cursor = page.next_cursor.clone();
            for item in page.items {
                if seen.insert(item.comment_id.clone()) {
                    all.items.push(item);
                }
            }
            if !all.has_more || all.next_cursor.is_empty() {
                break;
            }
            cursor = all.next_cursor.clone();
        }
        Ok(all)
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_danmaku_window() {
        let env = ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        // 首窗形态：medusa 风控降级每窗少量条目 + has_more=false（滚窗协议
        // 没有全量拉取路线，客户端随播放推进逐窗请求）；cursor/next_start
        // 是下一窗推进链的依据
        let w = fetch_danmaku_window_page(
            "7680669690372312089",
            "7680564528135818302",
            0,
            199000,
            "",
            &env,
        )
        .await
        .expect("首窗");
        println!(
            "[danmaku-win1] items={} has_more={} next_start={} cursor={:?}",
            w.items.len(),
            w.has_more,
            w.next_start_ms,
            &w.next_cursor[..w.next_cursor.len().min(200)]
        );
        assert!(!w.items.is_empty(), "这一集实测有弹幕");
        // 窗内按时间轴升序
        let mut sorted = w.items.clone();
        sorted.sort_by_key(|d| d.offset_ms);
        assert_eq!(w.items, sorted);
    }

    /// 评论区（ct=4/src=4）匿名可读性探测：旧弹幕形态被拒
    /// （103001），新形态（need_count/req_type/ch=18）应可用。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_comments_full() {
        let env = ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        match fetch_comments_all("7690197301075119166", "7690150906532219966", &env).await {
            Ok(page) => {
                println!(
                    "[comments-full] total={} items={} 首3: {:?}",
                    page.total,
                    page.items.len(),
                    &page.items[..page.items.len().min(3)]
                );
            }
            Err(e) => {
                println!("[comments-full] ERR {e}");
                panic!("评论区拉取失败：{e}");
            }
        }
    }

    /// 回复列表双维度探测。
    /// 锚点：剧评 7693600825641861912（book=7691228619774905368）
    /// 与评论 7693242834317837081（vid=7691249364097829913）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例（匿名可读，无需登录态）"]
    async fn probe_reply_lists() {
        let env = ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        let book = "7691228619774905368";

        let review = fetch_review_replies(book, "7693600825641861912", "", &env)
            .await
            .expect("剧评回复列表");
        println!(
            "[reply-review] total={} got={} has_more={} 首1: {:?}",
            review.total,
            review.items.len(),
            review.has_more,
            review.items.first().map(|r| (&r.reply_id, &r.text)),
        );
        if review.has_more {
            let page2 =
                fetch_review_replies(book, "7693600825641861912", &review.next_cursor, &env)
                    .await
                    .expect("剧评回复第2页");
            println!(
                "[reply-review#2] got={} cursor={}",
                page2.items.len(),
                page2.next_cursor
            );
        }

        let comment =
            fetch_comment_replies("7691249364097829913", book, "7693242834317837081", "", &env)
                .await
                .expect("评论回复列表");
        println!(
            "[reply-comment] total={} got={} has_more={} 首1: {:?}",
            comment.total,
            comment.items.len(),
            comment.has_more,
            comment.items.first().map(|r| (&r.reply_id, &r.text)),
        );
    }
}
