//! 互动操作：点赞 / 收藏 / 追更 / 发弹幕 / 发评论 / 互动状态
//!（口径详见 docs/hongguo-api-endpoints.md 第 9 节）。

mod model;

pub use model::{
    BookshelfEntry, FollowUpdateItem, FollowUpdatePage, InteractionItem, InteractionState,
};

pub use super::danmaku::{CommentItem, ReplyItem};

use crate::emoticon::CommentAttachment;

use serde_json::Value;

use super::client::ApiEnv;
use super::danmaku::LQ_API_ORIGIN;
use crate::params::interaction::{
    COMMIT_SOURCE_COMMENT, COMMIT_SOURCE_COMMENT_REPLY, COMMIT_SOURCE_DANMAKU,
    COMMIT_SOURCE_REVIEW_REPLY, DATA_TYPE_COMMENT, DATA_TYPE_DANMAKU, DATA_TYPE_DELETE,
    DATA_TYPE_REVIEW, GROUP_TYPE_COMMENT, GROUP_TYPE_REVIEW,
};
use hongguo_common::{AppError, AppResult};

pub(super) const COMMENT_ADD_PATH: &str = "/novel/commentapi/comment/add/v1/";

/// 埋点上下文（官方原样；服务端不校验，但对齐着带）。
/// 剧评发送（danmaku.rs send_series_review）复用同一素形态。
pub(crate) fn shark_param() -> Value {
    serde_json::json!({
        "enter_from": "MainFragmentActivity",
        "page_list": "MainFragmentActivity",
        "previous_page": "",
    })
}

/// 发弹幕（comment/add data_type=20）专用的埋点上下文：比素形态多
/// `aid` 与 `type=short_play`（评论/回复不带这俩）。
pub(super) fn shark_param_danmaku_add() -> Value {
    serde_json::json!({
        "aid": "8662",
        "type": "short_play",
        "enter_from": "MainFragmentActivity",
        "page_list": "MainFragmentActivity",
        "previous_page": "",
    })
}

/// 发弹幕（`data_type=20`）：`group_id`=分集 vid、`book_id`=series_id，
/// `offset_ms` 为视频内时间轴位置。返回服务端分配的 comment_id。
pub async fn send_danmaku(
    group_id: &str,
    book_id: &str,
    text: &str,
    offset_ms: u64,
    env: &ApiEnv,
) -> AppResult<String> {
    Ok(comment_add(
        CommentAddArgs {
            group_id,
            book_id,
            text,
            data_type: DATA_TYPE_DANMAKU,
            commit_source: COMMIT_SOURCE_DANMAKU,
            offset_ms,
            attachment: None,
        },
        env,
    )
    .await?
    .comment_id)
}

/// 发一条评论（`data_type=4`）。返回**服务端回显的完整评论对象**（与列表
/// 条目同构，前端直接插入列表）；`attachment` 在场时 body 顶层多 `image_data`。
pub async fn send_comment(
    group_id: &str,
    book_id: &str,
    text: &str,
    attachment: Option<&CommentAttachment>,
    env: &ApiEnv,
) -> AppResult<CommentItem> {
    comment_add(
        CommentAddArgs {
            group_id,
            book_id,
            text,
            data_type: DATA_TYPE_COMMENT,
            commit_source: COMMIT_SOURCE_COMMENT,
            offset_ms: 0,
            attachment,
        },
        env,
    )
    .await
}

/// reply/add 响应 → [`ReplyItem`]（`data.reply` 与 reply/list 条目同构；
/// 老响应形态兜 `data.comment_info`）。
fn parse_reply_response(value: &Value) -> AppResult<ReplyItem> {
    let raw = value
        .pointer("/data/reply")
        .or_else(|| value.pointer("/data/comment_info"))
        .ok_or_else(|| AppError::Media("回复响应缺少 reply 对象".into()))?;
    super::danmaku::parse_reply_item(raw)
        .ok_or_else(|| AppError::Media("回复响应对象字段不全".into()))
}

/// 回复一条评论或回复（独立端点 `reply/add`，不走 comment/add 带回复字段）。
/// 回复「回复」时 `reply_to_reply_id` 传被回复那条的回复 id。返回**服务端回显的完整回复对象**。
pub async fn send_reply(
    group_id: &str,
    book_id: &str,
    reply_to_comment_id: &str,
    reply_to_reply_id: Option<&str>,
    text: &str,
    env: &ApiEnv,
) -> AppResult<ReplyItem> {
    let mut payload = serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "has_aigc_content": false,
            "ignore_urge_rule": false,
            "log_extra": {},
            "offset": 0,
            "shark_param": shark_param(),
            "text_feature": {},
        },
        "commit_source": COMMIT_SOURCE_COMMENT_REPLY,
        "data_type": DATA_TYPE_COMMENT,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "reply_to_comment_id": reply_to_comment_id,
        "text": text,
    });
    if let Some(rr) = reply_to_reply_id.filter(|s| !s.is_empty()) {
        payload["reply_to_reply_id"] = serde_json::json!(rr);
    }
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造回复请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, REPLY_ADD_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析回复响应失败: {e}")))?;
    check_interact_code(&value)?;
    parse_reply_response(&value)
}

/// 互动接口的服务端拒绝用 `Auth` 变体透传原文（`error.auth` 刻意无译文，
/// 前端直接显示服务端 message——套通用文案反而误导）。
pub(super) fn check_interact_code(value: &Value) -> AppResult<()> {
    if value.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    let msg = value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/BaseResp/StatusMessage")
                .and_then(Value::as_str)
        })
        .unwrap_or("未知错误");
    Err(AppError::Auth(format!(
        "互动操作被服务端拒绝（{code}）: {msg}"
    )))
}

/// comment/add 双形态收口（@小果AI 提及是剧级形态，走 [`send_comment_mention`]）：
/// 弹幕与评论的 business_param 字段组不同，混用有 103001 风险。
struct CommentAddArgs<'a> {
    group_id: &'a str,
    book_id: &'a str,
    text: &'a str,
    data_type: i64,
    commit_source: i64,
    offset_ms: u64,
    /// 附图载荷（本地图片 / 表情图片）；纯文字评论为 None。
    attachment: Option<&'a CommentAttachment>,
}

async fn comment_add(args: CommentAddArgs<'_>, env: &ApiEnv) -> AppResult<CommentItem> {
    let CommentAddArgs {
        group_id,
        book_id,
        text,
        data_type,
        commit_source,
        offset_ms,
        attachment,
    } = args;
    let business_param = if data_type == DATA_TYPE_DANMAKU {
        serde_json::json!({
            "book_id": book_id,
            "ignore_urge_rule": false,
            "offset": offset_ms,
            "shark_param": shark_param_danmaku_add(),
        })
    } else {
        serde_json::json!({
            "book_id": book_id,
            "has_aigc_content": false,
            "ignore_urge_rule": false,
            "log_extra": {},
            "offset": offset_ms,
            "preset_text_id": "",
            "shark_param": shark_param(),
            "text_feature": {},
        })
    };
    let mut payload = serde_json::json!({
        "aid": 8662,
        "business_param": business_param,
        "commit_source": commit_source,
        "data_type": data_type,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "text": text,
    });
    if let Some(att) = attachment {
        payload["image_data"] = serde_json::json!([crate::emoticon::image_data_value(&att.image)]);
        if let Some(scene) = att.emoji_scene_type {
            payload["business_param"]["emoji_scene_type"] = serde_json::json!(scene);
        }
    }
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造弹幕/评论请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, COMMENT_ADD_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析弹幕/评论响应失败: {e}")))?;
    check_interact_code(&value)?;
    let raw = value
        .pointer("/data/comment_info")
        .ok_or_else(|| AppError::Media("发送响应缺少 comment_info".into()))?;
    super::danmaku::parse_comment_item(raw)
        .ok_or_else(|| AppError::Media("发送响应对象字段不全".into()))
}

const REPLY_ADD_PATH: &str = "/novel/commentapi/reply/add/v1/";
pub async fn send_review_reply(
    series_id: &str,
    reply_to_comment_id: &str,
    reply_to_reply_id: Option<&str>,
    text: &str,
    env: &ApiEnv,
) -> AppResult<ReplyItem> {
    let mut payload = serde_json::json!({
        "business_param": {
            "book_id": series_id,
            "from_famous_comment_id": 0,
            "has_aigc_content": false,
            "ignore_urge_rule": false,
            "is_confirm_request": false,
            "log_extra": {},
            "offset": 0,
            "read_item_cnt": 0,
            "score": 0,
            "support_para_audio_play": false,
            "text_feature": {},
            "video_is_muted": 0,
        },
        "commit_source": COMMIT_SOURCE_REVIEW_REPLY,
        "data_type": DATA_TYPE_REVIEW,
        "group_id": series_id,
        "group_type": GROUP_TYPE_REVIEW,
        "reply_to_comment_id": reply_to_comment_id,
        "text": text,
    });
    if let Some(rr) = reply_to_reply_id.filter(|s| !s.is_empty()) {
        payload["reply_to_reply_id"] = serde_json::json!(rr);
    }
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造剧评回复请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, REPLY_ADD_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评回复响应失败: {e}")))?;
    check_interact_code(&value)?;
    parse_reply_response(&value)
}

/// 删除自己的评论 / 剧评 / 回复。`service_id`：**2 = 剧评，4 = 评论/回复**；
/// `data_type` 恒 9（删除语义）。响应 `code==0` 即成功。
pub async fn delete_comment(comment_id: &str, service_id: i64, env: &ApiEnv) -> AppResult<()> {
    const DEL_PATH: &str = "/novel/commentapi/comment/del/v1/";
    let payload = serde_json::json!({
        "comment_id": comment_id,
        "data_type": DATA_TYPE_DELETE,
        "service_id": service_id,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造删除请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, DEL_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析删除响应失败: {e}")))?;
    check_interact_code(&value)
}

mod action;
mod list;

pub use action::{
    collect_series, collect_series_batch, digg_comment, digg_review, digg_video, follow_update,
};
pub use list::{fetch_bookshelf, fetch_follow_updates, fetch_interaction_state};
