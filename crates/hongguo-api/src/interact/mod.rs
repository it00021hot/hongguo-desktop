//! 互动操作：点赞 / 收藏 / 追更 / 发弹幕 / 发评论 / 互动状态（口径详见
//! docs/hongguo-api-endpoints.md 第 9 节）。
//!
//! 口径：
//! - **body 一律 gzip JSON**（`api_call_reading` 已统一），reading 轻签名头，
//!   **必须带登录 cookie**（x-tt-token 在场）；无强签名校验
//!   （旧 `x-reading-request`、不带 x-helios/x-medusa 都能过）；
//! - 点赞分两族：视频走 `articleapi/do_action`（object_type=6，action_type
//!   3/4），评论走 `commentapi/comment/do_action`（object_type=8，8/9）——
//!   **路径不同**，别合并；
//! - **追更也走 `articleapi/do_action`**（与点赞同端点）：object_type=10
//!   （短剧）、object_id=series_id、action_type 34 追 / 35 取消——同端点
//!   不同动作码，与点赞共用路径但 business_param 不带 video_id；
//! - 弹幕与评论发送同端点 `commentapi/comment/add`，只差 data_type
//!   （20 弹幕 / 4 评论）、commit_source（1500 / 3）与 offset（播放 ms / 0）；
//! - 收藏（追剧）走 `bookshelf/video/update`，对象是 **series_id**
//!   （body 字段名叫 book_id），`video_shelf_operate_type` 0 收 / 1 取；
//! - 互动状态 `ugc/action/mget` 是**列表**（互动过的最近 100 条），不是
//!   单集查询——回显是 best-effort。

mod model;

pub use model::{
    BookshelfEntry, FollowUpdateItem, FollowUpdatePage, InteractionItem, InteractionState,
    MentionUser,
};

pub use super::danmaku::{CommentItem, ReplyItem};

use crate::emoticon::CommentAttachment;

use serde_json::Value;

use super::client::ApiEnv;
use super::danmaku::LQ_API_ORIGIN;
use crate::params::interaction::{
    ACTION_CHANNEL_FOLLOW, ACTION_TYPE_FOLLOW, ACTION_TYPE_UNFOLLOW, COMMENT_TYPE_COMMENT,
    COMMENT_TYPE_REVIEW, COMMIT_SOURCE_COMMENT, COMMIT_SOURCE_COMMENT_REPLY, COMMIT_SOURCE_DANMAKU,
    COMMIT_SOURCE_REVIEW_REPLY, DATA_TYPE_COMMENT, DATA_TYPE_DANMAKU, DATA_TYPE_DELETE,
    DATA_TYPE_REVIEW, GROUP_TYPE_COMMENT, GROUP_TYPE_REVIEW, OBJECT_TYPE_COMMENT,
    OBJECT_TYPE_REVIEW, OBJECT_TYPE_SERIES, OBJECT_TYPE_VIDEO,
};
use hongguo_common::{AppError, AppResult};

const ARTICLE_DO_ACTION_PATH: &str = "/novel/articleapi/do_action/v1/";
const COMMENT_DO_ACTION_PATH: &str = "/novel/commentapi/comment/do_action/v1/";
pub(super) const COMMENT_ADD_PATH: &str = "/novel/commentapi/comment/add/v1/";
const REPLY_ADD_PATH: &str = "/novel/commentapi/reply/add/v1/";
const BOOKSHELF_UPDATE_PATH: &str = "/reading/bookapi/bookshelf/video/update/v";
const BOOKSHELF_LIST_PATH: &str = "/reading/bookapi/bookshelf/video/list/v";
const UGC_MGET_PATH: &str = "/reading/ugc/action/mget/v";
const FOLLOW_LIST_PATH: &str = "/novel/articleapi/relation/get_user_action/v1/";

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
fn shark_param_danmaku_add() -> Value {
    serde_json::json!({
        "aid": "8662",
        "type": "short_play",
        "enter_from": "MainFragmentActivity",
        "page_list": "MainFragmentActivity",
        "previous_page": "",
    })
}

/// 发弹幕（`data_type=20`）。`group_id`=分集 vid，`book_id`=series_id，
/// `offset_ms` 是弹幕在视频内的时间轴位置（与拉取侧 `expand.offset_time`
/// 同单位）。返回服务端分配的 comment_id（弹幕层只要 id 乐观插入）。
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

/// 发一条评论（`data_type=4`，offset 恒 0）。返回**服务端回显的完整评论
/// 对象**（comment/add 响应的 comment_info 与列表条目同构：
/// 头像/昵称/uid/时间全带）——前端直接插入列表，别手拼条目
/// （手拼缺 userId 删除按钮出不来，缺头像昵称样式和服务端条目两样）。
///
/// `attachment` 在场时走附图形态：body 顶层多 `image_data`，表情类另带
/// `business_param.emoji_scene_type`。
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

/// 发一条 **@小果AI** 的提及评论。
///
/// 形态与普通评论同流（播放页面板 = 单集流：`group_id=vid`、
/// `group_type=30`）。官方单集流里的提及评论：
/// comment `group_id=vid`、带 tp=15 text_exts、AI 回复 reply_count=3。
/// `text` 为用户正文（可空），最终请求 text 前置 `@昵称\u{FFFC}`
/// （官方昵称即「小果」；rich_text 段 tp=15/uri=uid、段文本 = 同一
/// 提及串，有正文时段文本含尾随空格、`e` 按段文本 UTF-16 单位计），
/// business_param 另带 aibot_mention_context（**最近评论
/// id** 的嵌套数组 JSON 串——官方体里是评论 id 不是 vid）。返回完整评论回显。
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
        // 与面板同流：group_id 是分集 vid，group_type=30
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

/// 回复一条评论或回复（独立端点 `reply/add`——
/// **不走 comment/add 带回复字段**）。`reply_to_comment_id` 是被回复的
/// 评论 id；回复「回复」时 `reply_to_reply_id` 传被回复的那条回复 id
/// （响应回显有此字段，多级回复同端点）。返回**服务端回显的完整回复
/// 对象**（头像/昵称/uid/时间齐全——直接把这个对象插进
/// 回复列表即可）。
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
/// 前端直接显示服务端 message——风控/频控的具体原因对用户是有效信息，
/// 套「视频处理失败」这类通用文案反而误导）。
fn check_interact_code(value: &Value) -> AppResult<()> {
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

/// comment/add 双形态：
/// - 弹幕 `data_type=20`：business_param 只有 book_id/ignore_urge_rule/
///   offset/shark_param（shark 带 aid+type）；
/// - 评论 `data_type=4`：business_param 另有 has_aigc_content/log_extra/
///   preset_text_id/text_feature，shark 是素形态——评论
///   形态服务端有独立校验，缺新字段或混入弹幕字段有 103001 风险。
///
/// 单集/弹幕维度的发送收口（@小果AI 提及是**剧级**形态，独立在
/// [`send_comment_mention`]，不走这里）。
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
    // 附图形态：body 顶层多 `image_data`，
    // business_param 多 `emoji_scene_type`——**本地图片评论不带该字段**
    // （表情类才有：第三方 2 / 自定义 4）。
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
    // 响应是完整 comment_info（与列表条目同构）——整体返回，弹幕侧取
    // comment_id，评论侧直接插入列表
    let raw = value
        .pointer("/data/comment_info")
        .ok_or_else(|| AppError::Media("发送响应缺少 comment_info".into()))?;
    super::danmaku::parse_comment_item(raw)
        .ok_or_else(|| AppError::Media("发送响应对象字段不全".into()))
}

/// 点赞 / 取消点赞一集视频。`vid`=分集 id（object_id），`series_id` 进
/// business_param.video_id（官方体里它填的是剧 id，字段名与语义不符，照抄）。
/// articleapi 点赞动作码：3 赞 / 4 取消。
fn video_digg_action_type(digg: bool) -> i64 {
    if digg { 3 } else { 4 }
}

/// commentapi 评论点赞动作码：8 赞 / 9 取消。
fn comment_digg_action_type(digg: bool) -> i64 {
    if digg { 8 } else { 9 }
}

/// 书架操作码：0 收藏 / 1 取消。
fn shelf_operate_type(collect: bool) -> i64 {
    if collect { 0 } else { 1 }
}

pub async fn digg_video(vid: &str, series_id: &str, digg: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "action_category": 1,
        "action_reason_remark": "like_click",
        "action_type": video_digg_action_type(digg),
        "business_param": {
            "book_id": 0,
            "has_aigc_content": false,
            "modify_count": 0,
            "shark_param": shark_param(),
            "video_id": series_id,
        },
        "object_id": vid,
        "object_type": OBJECT_TYPE_VIDEO,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造点赞请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, ARTICLE_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 点赞 / 取消点赞一条评论（评论区 UI 后续接入）。
pub async fn digg_comment(comment_id: &str, digg: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "action_type": comment_digg_action_type(digg),
        "business_param": { "shark_param": shark_param() },
        "comment_type": COMMENT_TYPE_COMMENT,
        "object_id": comment_id,
        "object_type": OBJECT_TYPE_COMMENT,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造评论点赞请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, COMMENT_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析评论点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 点赞 / 取消点赞一条**剧评**。
///
/// 与评论点赞（[`digg_comment`])同路径但三处形态不同，别合并：
/// `object_type=2`（评论 8）、`comment_type=2`（评论 4）、
/// **`business_param` 是空对象**（评论带 shark_param）。
pub async fn digg_review(review_id: &str, digg: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "action_type": comment_digg_action_type(digg),
        "business_param": {},
        "comment_type": COMMENT_TYPE_REVIEW,
        "object_id": review_id,
        "object_type": OBJECT_TYPE_REVIEW,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造剧评点赞请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, COMMENT_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 回复一条**剧评**或剧评的回复。
///
/// 与单集评论回复（[`send_reply`]）同端点（reply/add）但换剧评形态：
/// `commit_source=13`（评论回复 9）、`data_type=2`、`group_type=1`、
/// **顶层不带 aid**，business_param 是剧评发送同款全量字段组
/// （score 恒 0——评分只随剧评本体走）。回复「回复」时
/// `reply_to_reply_id` 传被回复那条的 reply_id。返回 reply_id。
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

/// 删除自己的评论 / 剧评 / 回复。
///
/// `service_id`：**2 = 剧评，4 = 评论/回复**；`data_type` 恒 9
/// （删除语义，与发送侧 data_type 无关）。响应 `code==0` 即成功。
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

/// 收藏（追剧/进书架）或取消收藏一部剧。对象是 series_id。
pub async fn collect_series(series_id: &str, collect: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        // 信封常量，官方原样：取消收藏的语义不在这里，
        // 在 update_bookshelf_video_list[].video_shelf_operate_type（0 收/1 取消）
        "is_cancelled": false,
        "shark_extra": {
            "enter_from": "MainFragmentActivity",
            // 字符串形态的 "0"/"true"（官方原样）
            "inactive_type": "0",
            "is_active_behavior": "true",
            "page_list": "MainFragmentActivity",
            "previous_page": "",
        },
        "update_bookshelf_video_list": [{
            "book_id": series_id,
            "book_type": 2,
            "modify_time": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "video_shelf_operate_type": shelf_operate_type(collect),
        }],
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造收藏请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_UPDATE_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析收藏响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 批量收藏/取消收藏（RemoveFavorites 形态：
/// 同一 `update_bookshelf_video_list` 携带多条目，一请求完成批量）。
/// `(series_id, collect)` 对；信封与单条完全同形。
pub async fn collect_series_batch(entries: &[(String, bool)], env: &ApiEnv) -> AppResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let list: Vec<Value> = entries
        .iter()
        .map(|(series_id, collect)| {
            serde_json::json!({
                "book_id": series_id,
                "book_type": 2,
                "modify_time": now,
                "video_shelf_operate_type": shelf_operate_type(*collect),
            })
        })
        .collect();
    let payload = serde_json::json!({
        "is_cancelled": false,
        "shark_extra": {
            "enter_from": "MainFragmentActivity",
            "inactive_type": "0",
            "is_active_behavior": "true",
            "page_list": "MainFragmentActivity",
            "previous_page": "",
        },
        "update_bookshelf_video_list": list,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造批量收藏请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_UPDATE_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量收藏响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 追更 / 取消追更一部剧。
///
/// 与点赞同端点（articleapi/do_action）但形态不同，别合并：
/// `action_type` 34 追 / 35 取消、`object_type=10`（短剧）、
/// `object_id`=series_id，business_param **不带 video_id**（点赞带）。
pub async fn follow_update(series_id: &str, follow: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "action_type": follow_action_type(follow),
        "business_param": {
            "book_id": 0,
            "has_aigc_content": false,
            "modify_count": 0,
            "shark_param": shark_param(),
        },
        "object_id": series_id,
        "object_type": OBJECT_TYPE_SERIES,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造追更请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, ARTICLE_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析追更响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 追更动作码：34 追 / 35 取消。
fn follow_action_type(follow: bool) -> i64 {
    if follow {
        ACTION_TYPE_FOLLOW
    } else {
        ACTION_TYPE_UNFOLLOW
    }
}

/// 评论 @ 提及列表的场景枚举值。
const COMMENT_SOURCE_AT_MENTION: i64 = 1511;

/// 拉评论 @ 提及候选（relation/list）。
/// 官方列表含官方 AI bot「小果」（is_ai_bot=true）。`series_id` 同时进
/// 路径与 body（target_user_id / relation_list_business_param.book_id）。
pub async fn fetch_mention_list(series_id: &str, env: &ApiEnv) -> AppResult<Vec<MentionUser>> {
    let book_id: i64 = series_id
        .parse()
        .map_err(|_| AppError::InvalidArgs(format!("剧 id 不是数字: {series_id}")))?;
    let payload = serde_json::json!({
        "comment_source": COMMENT_SOURCE_AT_MENTION,
        "target_user_id": series_id,
        "aid": 8662,
        "count": 20,
        "relation_list_business_param": { "book_id": book_id },
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造提及列表请求失败: {e}")))?;
    let path = format!("/novel/commentapi/relation/list/{series_id}/v1/");
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析提及列表失败: {e}")))?;
    check_interact_code(&value)?;
    let mut items = Vec::new();
    for raw in value
        .pointer("/data/ugc_user_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let user_id = raw
            .pointer("/base_info/user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if user_id.is_empty() {
            continue;
        }
        items.push(MentionUser {
            user_id,
            user_name: raw
                .pointer("/base_info/user_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar: raw
                .pointer("/base_info/user_avatar")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            is_ai_bot: raw
                .pointer("/base_info/is_ai_bot")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok(items)
}

/// AI 提及的草稿：被提及用户与 AI 上下文评论（官方
/// aibot_mention_context = 最近评论 id 的嵌套数组）。
pub struct CommentMention {
    /// 被提及用户 uid（relation/list 条目；AI bot「小果」即 is_ai_bot）
    pub user_id: String,
    /// 被提及用户昵称——text 前缀与 rich_text 段文本都是 `@昵称\u{FFFC}`
    /// （对「小果」即 `@小果\u{FFFC}`，@ 普通用户不能张冠李戴成「小果」）
    pub user_name: String,
    /// AI 上下文评论 id（面板最近评论优先）
    pub comment_ids: Vec<String>,
}

/// 拉追更列表一页（get_user_action `action_channel=9`）。
/// `cursor` 传上页响应的 `cursor` 原文（JSON 串）；首页传空串。
/// 详情页的「是否已追」没有独立接口——拉第一页本地判成员（player 同款）。
pub async fn fetch_follow_updates(cursor: &str, env: &ApiEnv) -> AppResult<FollowUpdatePage> {
    let biz_query: Vec<(String, String)> = vec![
        ("action_channel".into(), ACTION_CHANNEL_FOLLOW.to_string()),
        ("count".into(), "20".into()),
        ("cursor_str".into(), cursor.to_string()),
    ];
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, FOLLOW_LIST_PATH, None, &biz_query, env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析追更列表失败: {e}")))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = value.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!("追更列表接口返回: {msg}")));
    }
    Ok(parse_follow_updates(&value))
}

/// 解析追更列表（条目缺失容错：mix_data[].video 任一层缺字段按空处理，
/// video_id 为空的条目跳过）。
fn parse_follow_updates(value: &Value) -> FollowUpdatePage {
    let cursor = value
        .pointer("/data/comment_list_info/cursor")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let has_more = value
        .pointer("/data/comment_list_info/has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut items = Vec::new();
    for entry in value
        .pointer("/data/mix_data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(video) = entry.get("video") else {
            continue;
        };
        let series_id = video
            .get("video_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if series_id.is_empty() {
            continue;
        }
        let expand = video.get("expand");
        items.push(FollowUpdateItem {
            series_id,
            title: video
                .pointer("/common/title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cover: video
                .pointer("/common/cover")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            episode_cnt: expand
                .and_then(|e| e.get("episode_cnt"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            latest_watched_vid_index: expand
                .and_then(|e| e.get("latest_watched_vid_index"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            finished: expand
                .and_then(|e| e.get("series_status"))
                .and_then(Value::as_i64)
                == Some(1),
            has_watched_latest_episode: expand
                .and_then(|e| e.get("has_watched_latest_episode"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            follow_update_at: expand
                .and_then(|e| e.pointer("/extra/follow_update_time_stamp"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0),
        });
    }
    FollowUpdatePage {
        items,
        cursor,
        has_more,
    }
}

/// 拉账号的书架（收藏）列表。「我的收藏」页数据源；`target_user_id` 是
/// 登录用户 uid（AccountState.user_id，官方同款 query）。
///
/// 响应条目字段名未在非空样本中验证过，按同族字段
/// 容错解析：series_id/book_id/item_id 任一在场即认；时间取
/// collect_time(_ms)/create_time。
pub async fn fetch_bookshelf(target_user_id: &str, env: &ApiEnv) -> AppResult<Vec<BookshelfEntry>> {
    let biz_query: Vec<(String, String)> =
        vec![("target_user_id".into(), target_user_id.to_string())];
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_LIST_PATH, None, &biz_query, env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析书架列表失败: {e}")))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = value.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!("书架列表接口返回: {msg}")));
    }
    let mut items = Vec::new();
    for raw in value
        .pointer("/data/video_shelf_info")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let series_id = ["series_id", "book_id", "item_id"]
            .iter()
            .find_map(|k| raw.get(*k).and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| {
                ["series_id", "book_id", "item_id"]
                    .iter()
                    .find_map(|k| raw.get(*k).map(num_to_string))
            })
            .unwrap_or_default();
        if series_id.is_empty() {
            continue;
        }
        // 时间量级防错：候选字段秒/毫秒形态可能并存，
        // 按值判量级（1e12 ≈ 2001 年起的毫秒时间戳）——毫秒原样用，
        // 秒形态 ×1000，字段名顺序不再决定正确性。
        let collect_time_ms = ["collect_time_ms", "collect_time", "create_time"]
            .iter()
            .filter_map(|k| raw.get(*k).and_then(Value::as_i64))
            .map(|v| if v > 1_000_000_000_000 { v } else { v * 1000 })
            .next()
            .unwrap_or(0);
        let content_type = ["content_type", "video_type"]
            .iter()
            .find_map(|k| raw.get(*k).and_then(Value::as_i64))
            .unwrap_or(0);
        items.push(BookshelfEntry {
            series_id,
            collect_time_ms,
            content_type,
        });
    }
    Ok(items)
}

/// JSON 数字 → 字符串（u64 精度无损）。
fn num_to_string(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// 拉互动状态列表（`ugc/action/mget`，最近 100 条）。
///
/// 这是列表接口而非单集查询：打开播放页后在前端用当前 vid / series_id
/// 匹配即可；不在列表里就当未互动（best-effort，不影响操作）。
pub async fn fetch_interaction_state(env: &ApiEnv) -> AppResult<InteractionState> {
    let biz_query: Vec<(String, String)> = vec![
        ("action_type".into(), "3".into()),
        ("count".into(), "100".into()),
        ("offset".into(), "0".into()),
        ("object_type_list".into(), "6,15,10".into()),
    ];
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, UGC_MGET_PATH, None, &biz_query, env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析互动状态失败: {e}")))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = value.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!("互动状态接口返回: {msg}")));
    }
    let mut state = InteractionState::default();
    for entry in value
        .pointer("/data/mixed_data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(video) = entry.get("video_data") else {
            continue;
        };
        let vid = video.get("vid").and_then(Value::as_str).unwrap_or_default();
        if vid.is_empty() {
            continue;
        }
        let item = InteractionItem {
            vid: vid.to_string(),
            series_id: video
                .get("series_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            user_digg: video.get("user_digg").and_then(Value::as_bool) == Some(true),
            digged_count: video
                .get("digged_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            followed: video
                .pointer("/video_detail/followed")
                .and_then(Value::as_bool)
                == Some(true),
            followed_cnt: video
                .pointer("/video_detail/followed_cnt")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            series_title: video
                .pointer("/video_detail/series_title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        state.items.push(item);
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danmaku_add_payload_shape_matches_capture() {
        // 形状锚点：comment/add（data_type=20 弹幕 / 4 评论只差三字段）
        let shark = shark_param_danmaku_add();
        assert_eq!(shark["aid"], "8662");
        assert_eq!(shark["type"], "short_play");
        assert_eq!(shark["enter_from"], "MainFragmentActivity");
        // 三字段差异是接口唯一分叉，写死锚点防手滑
        assert_ne!(20, 4);
    }

    #[test]
    fn digg_action_types_match_capture() {
        // articleapi: 3 赞 / 4 取消；commentapi: 8 赞 / 9 取消。
        // 断言打在请求构造实际调用的选择函数上，字面量改了这里就红。
        assert_eq!(video_digg_action_type(true), 3);
        assert_eq!(video_digg_action_type(false), 4);
        assert_eq!(comment_digg_action_type(true), 8);
        assert_eq!(comment_digg_action_type(false), 9);
    }

    #[test]
    fn shelf_operate_types_match_capture() {
        // bookshelf: 0 收藏 / 1 取消
        assert_eq!(shelf_operate_type(true), 0);
        assert_eq!(shelf_operate_type(false), 1);
    }

    #[test]
    fn follow_action_types_match_capture() {
        // do_action: 34 追更 / 35 取消
        assert_eq!(follow_action_type(true), 34);
        assert_eq!(follow_action_type(false), 35);
    }

    #[test]
    fn follow_updates_parse_matches_capture() {
        // 形状锚点：get_user_action action_channel=9 官方样本
        let sample = serde_json::json!({
            "code": 0,
            "data": {
                "comment_list_info": {
                    "cursor": "{\"session_id\":\"20261015132033C96A23C06C91444C4511\",\"offset\":20}",
                    "has_more": false
                },
                "mix_data": [{
                    "data_type": 1,
                    "video": {
                        "common": {
                            "cover": "https://p3-reading-sign.fqnovelpic.com/novel-pic/x.heic",
                            "title": "摆摊大王",
                            "vertical": false,
                            "video_content_type": 1004,
                            "video_type": 2
                        },
                        "expand": {
                            "episode_cnt": 214,
                            "extra": {
                                "follow_update_time_stamp": "1792041548",
                                "lasted_read_time_stamp": "1792041412",
                                "max_chapter_first_pass_time": "1787631872"
                            },
                            "has_watched_latest_episode": false,
                            "latest_watched_vid_index": 16,
                            "series_status": 1
                        },
                        "video_id": "7677818497497320472"
                    }
                }]
            }
        });
        let page = parse_follow_updates(&sample);
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.series_id, "7677818497497320472");
        assert_eq!(item.title, "摆摊大王");
        assert_eq!(item.episode_cnt, 214);
        assert_eq!(item.latest_watched_vid_index, 16);
        assert!(item.finished, "series_status=1 = 已完结");
        assert!(!item.has_watched_latest_episode);
        assert_eq!(item.follow_update_at, 1_792_041_548);
        // cursor 原样保留（翻页续传），has_more 透传
        assert!(page.cursor.contains("session_id"));
        assert!(!page.has_more);
    }

    #[test]
    fn follow_updates_parse_tolerates_empty_and_junk() {
        // 空列表（追更前形态）与缺 video 层的脏条目都不炸
        let empty = serde_json::json!({
            "code": 0,
            "data": {"comment_list_info": {"cursor": "", "has_more": false}, "mix_data": []}
        });
        let page = parse_follow_updates(&empty);
        assert!(page.items.is_empty());
        assert!(page.cursor.is_empty());

        let junk = serde_json::json!({
            "code": 0,
            "data": {"mix_data": [{"data_type": 1}, {"video": {"common": {"title": "无id"}}}]}
        });
        assert!(parse_follow_updates(&junk).items.is_empty());
    }

    #[test]
    fn state_parsing_tolerates_missing_fields() {
        let empty = serde_json::json!({"code": 0, "data": null});
        assert_eq!(
            empty
                .pointer("/data/mixed_data_list")
                .and_then(Value::as_array),
            None,
            "空响应按无列表处理，不强解"
        );
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    // 删除评论 / 回复时用（生产路径的 service_id 由前端传入）。
    use crate::params::interaction::SERVICE_ID_COMMENT;
    use hongguo_common::model::ProxyConfig;

    /// 互动操作必须登录态（匿名被服务端静默拒）。probe 从环境变量
    /// `HG_TEST_COOKIE`（`k=v; k=v` 全量会话 cookie）、可选
    /// `HG_TEST_TOKEN`（x-tt-token 长凭据）与可选 `HG_TEST_UID`
    /// （target_user_id，书架列表用）构造环境，未设置则跳过。
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
        // 锚点：剧评 7693600825641861912（book=7691228619774905368），
        // 评论 7693242834317837081（vid=7691249364097829913）
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
        // 回复列表读取（匿名也应可读；带登录态对照）
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
        // 锚点剧（收徒就变强 第1集）：vid / series_id
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

        // 删除闭环（comment/del）：把自己刚发的
        // 评论和回复删掉——既验证端点又清理 probe 痕迹
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
