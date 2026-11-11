//! 互动操作：点赞 / 收藏 / 追更 / 发弹幕 / 发评论 / 互动状态
//!（口径详见 docs/hongguo-api-endpoints.md 第 9 节）。

mod model;

pub use model::{
    BookshelfEntry, CommentMention, FollowUpdateItem, FollowUpdatePage, InteractionItem,
    InteractionState, MentionUser,
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

/// 发一条 **@小果AI** 的提及评论（`text` 为用户正文，可空）。返回完整评论回显。
pub async fn send_comment_mention(
    group_id: &str,
    book_id: &str,
    mention: &CommentMention,
    text: &str,
    env: &ApiEnv,
) -> AppResult<CommentItem> {
    let body = text.trim();
    let nick = mention.user_name.trim();
    let segment = format!("@{nick}\u{FFFC}");
    let units = segment.encode_utf16().count() as i64;
    let (final_text, segment_text, segment_end) = if body.is_empty() {
        (segment.clone(), segment, units)
    } else {
        (
            format!("{segment} {body}"),
            format!("{segment} "),
            units + 1,
        )
    };
    let ctx = serde_json::to_string(
        &mention
            .comment_ids
            .iter()
            .map(|id| serde_json::json!([id]))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let payload = serde_json::json!({
        "aid": 8662,
        "commit_source": COMMIT_SOURCE_COMMENT,
        "data_type": DATA_TYPE_COMMENT,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "text": final_text,
        "rich_text": [{
            "s": 0,
            "e": segment_end,
            "tp": 15,
            "uri": mention.user_id,
            "text": segment_text,
        }],
        "business_param": {
            "ignore_urge_rule": false,
            "text_feature": {},
            "book_id": book_id,
            "shark_param": shark_param(),
            "has_aigc_content": false,
            "offset": 0,
            "preset_text_id": "",
            "aibot_mention_context": ctx,
            "log_extra": {},
        },
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造提及评论请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, COMMENT_ADD_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析提及评论响应失败: {e}")))?;
    check_interact_code(&value)?;
    let raw = value
        .pointer("/data/comment_info")
        .ok_or_else(|| AppError::Media("提及评论响应缺少 comment_info".into()))?;
    super::danmaku::parse_comment_item(raw)
        .ok_or_else(|| AppError::Media("提及评论响应对象字段不全".into()))
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
pub use list::{
    fetch_bookshelf, fetch_follow_updates, fetch_interaction_state, fetch_mention_list,
};

#[cfg(test)]
use action::{
    comment_digg_action_type, follow_action_type, shelf_operate_type, video_digg_action_type,
};
#[cfg(test)]
use list::parse_follow_updates;

#[cfg(test)]
#[path = "interact_tests.rs"]
mod tests;

#[cfg(test)]
mod probe {
    use super::*;
    use crate::params::interaction::SERVICE_ID_COMMENT;
    use hongguo_common::model::ProxyConfig;

    /// probe 环境：`HG_TEST_COOKIE`（全量会话 cookie）必设，可选 `HG_TEST_TOKEN` / `HG_TEST_UID`，未设则跳过。
    fn login_env() -> Option<ApiEnv> {
        let cookie = std::env::var("HG_TEST_COOKIE").ok()?;
        let mut env = ApiEnv::anonymous(ProxyConfig::default());
        env.cookie = Some(cookie);
        env.x_tt_token = std::env::var("HG_TEST_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        Some(env)
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例（需要 HG_TEST_COOKIE 登录态；无 HG_TEST_TOKEN 时顺便验证无 token 的互动可行性）"]
    async fn probe_review_interactions() {
        let Some(env) = login_env() else {
            println!("[review-interact] 未设 HG_TEST_COOKIE，跳过");
            return;
        };
        println!(
            "[review-interact] x_tt_token={}（空 = 验证无 token 形态）",
            if env.x_tt_token.is_some() {
                "在场"
            } else {
                "缺失"
            }
        );
        let book = "7691228619774905368";
        let review = "7693600825641861912";
        let comment = "7693242834317837081";

        match digg_review(review, true, &env).await {
            Ok(()) => println!("[review-interact] 剧评点赞 OK"),
            Err(e) => println!("[review-interact] 剧评点赞 ERR: {e}"),
        }
        match digg_review(review, false, &env).await {
            Ok(()) => println!("[review-interact] 剧评取消点赞 OK"),
            Err(e) => println!("[review-interact] 剧评取消点赞 ERR: {e}"),
        }
        match digg_comment(comment, true, &env).await {
            Ok(()) => println!("[review-interact] 评论点赞 OK"),
            Err(e) => println!("[review-interact] 评论点赞 ERR: {e}"),
        }
        match digg_comment(comment, false, &env).await {
            Ok(()) => println!("[review-interact] 评论取消点赞 OK"),
            Err(e) => println!("[review-interact] 评论取消点赞 ERR: {e}"),
        }
        match send_review_reply(book, review, None, "probe 剧评回复", &env).await {
            Ok(reply) => {
                println!(
                    "[review-interact] 剧评回复 OK id={} user={} text={}",
                    reply.reply_id, reply.user_name, reply.text
                );
                match delete_comment(&reply.reply_id, SERVICE_ID_COMMENT, &env).await {
                    Ok(()) => println!("[review-interact] 回复删除 OK（清理痕迹）"),
                    Err(e) => println!("[review-interact] 回复删除 ERR: {e}"),
                }
            }
            Err(e) => println!("[review-interact] 剧评回复 ERR: {e}"),
        }
        let page = crate::danmaku::fetch_review_replies(book, review, "", &env)
            .await
            .expect("剧评回复列表");
        println!(
            "[review-interact] 剧评回复列表 total={} got={}",
            page.total,
            page.items.len()
        );
    }

    #[tokio::test]
    #[ignore = "直连真实接口的探测用例（需要 HG_TEST_COOKIE 登录态）"]
    async fn probe_interaction_roundtrip() {
        let Some(env) = login_env() else {
            println!("[interact] 未设 HG_TEST_COOKIE，跳过");
            return;
        };
        let vid = "7692043112050330648";
        let series = "7692006324439092248";

        digg_video(vid, series, true, &env).await.expect("点赞");
        digg_video(vid, series, false, &env)
            .await
            .expect("取消点赞");

        let cid = send_danmaku(vid, series, "probe 弹幕", 1000, &env)
            .await
            .expect("发弹幕");
        println!("[interact] danmaku comment_id={cid}");

        let ccid = send_comment(vid, series, "probe 评论", None, &env)
            .await
            .expect("发评论（播放页单集流形态）");
        println!("[interact] comment comment_id={}", ccid.comment_id);

        let reply = send_reply(vid, series, &ccid.comment_id, None, "probe 回复", &env)
            .await
            .expect("回复（reply/add 独立端点）");
        println!("[interact] reply reply_id={}", reply.reply_id);

        delete_comment(&ccid.comment_id, SERVICE_ID_COMMENT, &env)
            .await
            .expect("删除评论");
        println!("[interact] comment {} deleted", ccid.comment_id);
        delete_comment(&reply.reply_id, SERVICE_ID_COMMENT, &env)
            .await
            .expect("删除回复");
        println!("[interact] reply {} deleted", reply.reply_id);

        collect_series(series, true, &env).await.expect("收藏");
        collect_series(series, false, &env).await.expect("取消收藏");

        if let Ok(uid) = std::env::var("HG_TEST_UID") {
            let shelf = fetch_bookshelf(&uid, &env).await.expect("书架列表");
            println!("[interact] bookshelf {} 条", shelf.len());
        }

        let state = fetch_interaction_state(&env).await.expect("互动状态");
        println!(
            "[interact] items={} digged={} followed={}",
            state.items.len(),
            state.items.iter().filter(|i| i.user_digg).count(),
            state.items.iter().filter(|i| i.followed).count()
        );
    }
}
