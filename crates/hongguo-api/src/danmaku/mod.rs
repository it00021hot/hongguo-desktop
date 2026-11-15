//! 弹幕：commentapi 的 comment/list（comment_type=20）滚窗拉取。
//! 评论区 / 剧评 / 回复链在 `comments`，经本模块 re-export 对外。

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

use serde::Serialize;
use serde_json::Value;

use super::client::ApiEnv;
use crate::params::interaction::{
    COMMENT_TYPE_DANMAKU, GROUP_TYPE_COMMENT, LIST_SOURCE_DANMAKU, SERVER_CHANNEL_DANMAKU,
};
use hongguo_common::{AppError, AppResult};

/// reading 系 API 主机（commentapi 只在这个主机上存在）。
pub const LQ_API_ORIGIN: &str = crate::client::LQ_API_ORIGIN;

/// 弹幕窗请求的 `need_danmaku_guide_type`（官方常态值）。
const DANMAKU_GUIDE_TYPE: i64 = 2;

/// 一窗弹幕。弹幕必须随播放进度滚窗（官方唯一形态，没有「一次拉全」路线）；
/// 请求侧 `business_param.playlet_item_duration` 仍须传真值毫秒。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DanmakuWindowPage {
    pub items: Vec<Danmaku>,
    /// 还有下一窗/下一页。
    pub has_more: bool,
    /// 下一窗请求**原样回传**的 cursor（服务端下发，客户端不改写）。
    pub next_cursor: String,
    /// 下一窗的播放位置起点（ms）＝本窗 cursor 的 `end_offset_time`，
    /// 请求侧 `business_param.start_offset_time` 按它推进。
    pub next_start_ms: u64,
}

/// 请求体（business_param 只有官方这四键）。
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

/// 拉一窗弹幕（官方滚窗协议）：回传上一响应 cursor、start 推进到其 end 拉下一窗，
/// 窗内分页 has_more 继续。`item_duration_ms` 是本集真实时长，解析不出传 0 也能拉。
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

/// 弹幕响应 → 一窗。
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
