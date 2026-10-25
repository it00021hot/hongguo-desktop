//! 评论区 / 剧评 / 回复链：comment/list 的评论与剧评两种形态，加 reply/list
//! 的两维度回复列表。弹幕形态（comment_type=20 的窗口拉取）见 [`super`]。

use serde_json::Value;

use super::{
    CommentItem, CommentPage, CommentTagStat, CreatorProfile, CreatorWork, CreatorWorksPage,
    LQ_API_ORIGIN, ReplyItem, ReplyPage, SeriesReviewPage,
};
use crate::client::ApiEnv;
use crate::params::interaction::{
    COMMENT_TYPE_COMMENT, COMMENT_TYPE_REVIEW, COMMENT_TYPE_REVIEW_SEND, COMMIT_SOURCE_REVIEW,
    DATA_TYPE_REVIEW, GROUP_TYPE_COMMENT, GROUP_TYPE_REVIEW, LIST_SOURCE_COMMENT,
    LIST_SOURCE_COMMENT_REPLIES, LIST_SOURCE_PROFILE_WORKS, LIST_SOURCE_REVIEW,
    LIST_SOURCE_REVIEW_REPLIES, PROFILE_SUB_TAB_WORKS, PROFILE_TAB_WORKS, SERVER_CHANNEL_COMMENT,
    SERVER_CHANNEL_REVIEW,
};
use hongguo_common::{AppError, AppResult};

/// 播放页评论面板请求体。
///
/// 2026-10-16 官方实包重锁（flows-official-follow.jsonl:3308，播放页
/// ShortSeriesActivity 面板实测）：面板是**单集维度**——`group_id=vid`、
/// `group_type=30`、`server_channel=18`、`comment_source=4`、`count=10`、
/// `business_param.sort="video_innerflow"`；`need_count` 仅首屏 true
/// （total 回传本集评论总数，互动栏角标同源），翻页置 false 并回传
/// 响应下发的会话游标。剧集维度（group=series / channel=27）是沉浸流
/// 信息流面板的另一条流，只有流内评论（本集评论不在其中），勿混用。
pub(super) fn comments_payload(group_id: &str, book_id: &str, cursor: &str) -> Value {
    let mut payload = serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_count": cursor.is_empty(),
            "req_type": 0,
            "sort": "video_innerflow",
            "start_offset_time": 0,
            "playlet_consume_duration_ms": 0,
            "playlet_item_consume_duration_ms": 0,
            "outflow_comment_count": 0,
            "is_last_episode": false,
        },
        "comment_source": LIST_SOURCE_COMMENT,
        "comment_type": COMMENT_TYPE_COMMENT,
        "compliance_status": 0,
        "count": 10,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "server_channel": SERVER_CHANNEL_COMMENT,
        "sort": 1,
    });
    // 首屏不带 cursor 键（官方同款），翻页才回传响应下发的游标原样串
    if !cursor.is_empty() {
        payload["cursor"] = Value::String(cursor.to_string());
    }
    payload
}

/// 拉播放页评论区的一页（`group_id`=vid、`book_id`=series_id；cursor 传
/// 上一页返回的游标，首页传空）。
pub async fn fetch_comments_page(
    group_id: &str,
    book_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<CommentPage> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&comments_payload(group_id, book_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析评论响应失败: {e}")))?;
    check_comment_code(&v)?;
    parse_comment_page(&v)
}

/// 剧级评论（详情页「剧评」，整部剧一条线）。
///
/// 2026-10-07 抓详情页锁定：同一端点换**形态**——`group_id` 是
/// **series_id**（不是 vid）、`group_type=1`（书/剧维度）、`comment_source=1`、
/// `comment_type=2`、`server_channel=34`；business_param 是另一组全量字段
/// （照抄抓包，审计纪律）。响应与单集评论同构（common_list_info.total /
/// data_list），解析复用。
pub(super) fn series_comments_payload(group_id: &str, cursor: &str) -> Value {
    let mut payload = serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": group_id,
            "comment_sort_debug": false,
            "end_offset_time": 0,
            "hit_interaction_data_migration_ab": false,
            "is_last_episode": false,
            "item_count": 0,
            "max_item_count": 0,
            "need_count": true,
            "need_danmaku_occlusion_face_data": false,
            "outflow_comment_count": 0,
            "para_index": 0,
            "playlet_consume_duration_ms": 0,
            "playlet_item_consume_duration_ms": 0,
            "playlet_item_duration": 0,
            "read_item_count": 0,
            "req_type": 0,
            "start_offset_time": 0,
        },
        "comment_source": LIST_SOURCE_REVIEW,
        "comment_type": COMMENT_TYPE_REVIEW,
        "count": 10,
        "group_id": group_id,
        "group_type": GROUP_TYPE_REVIEW,
        "server_channel": SERVER_CHANNEL_REVIEW,
        "sort": 1,
    });
    // 首屏不带 cursor 键（2026-10-10 抓包实锤：cursor 缺省，翻页才回传
    // 响应下发的 `{"session_id":..,"offset":N}` JSON 串原样）
    if !cursor.is_empty() {
        payload["cursor"] = Value::String(cursor.to_string());
    }
    payload
}

/// 拉剧级评论的一页（详情页「剧评」tab 数据源 + 头部评分）。
pub async fn fetch_series_comments_page(
    series_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<SeriesReviewPage> {
    let path = format!("/novel/commentapi/comment/list/{series_id}/v1/");
    let body = serde_json::to_vec(&series_comments_payload(series_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评响应失败: {e}")))?;
    check_comment_code(&v)?;
    let page = parse_comment_page(&v)?;
    // credibility_score 线上是**数字**形态（8.3），字符串/数字两种都兜
    let score = v
        .pointer("/data/extra/credibility_score")
        .map(|x| match x {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let score_cnt = v
        .pointer("/data/extra/credibility_score_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let tags = v
        .pointer("/data/extra/book_info/tags")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    // 剧评标签统计（extra.filter_tag，「修仙世界观宏大 26」pill 行，
    // 2026-10-10 抓包锁定；剧均评分本体 = 上面的 credibility_score）
    let tag_stats = v
        .pointer("/data/extra/filter_tag")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|t| CommentTagStat {
                    tag_name: t
                        .get("tag_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    count: t.get("count").and_then(Value::as_i64).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SeriesReviewPage {
        page,
        score,
        score_cnt,
        tags,
        tag_stats,
    })
}

/// 发剧评（详情页「剧评」tab 的评论框，带评分）。
///
/// 参数照抓包逐字段对齐（2026-10-10，用户实测发满分
/// 剧评「好看」抓包）：剧评形态是 **data_type=2 / commit_source=12 /
/// comment_type=0**，评分放在 business_param.score（**十分制**，5 星
/// ×2）；列表侧参数（comment_source/comment_type/server_channel）与
/// 回复侧 commit_source=9 都会被服务端 103008「无社区功能」拒收。
/// 返回服务端分配的 comment_id。
pub async fn send_series_review(
    series_id: &str,
    text: &str,
    score: i64,
    env: &ApiEnv,
) -> AppResult<CommentItem> {
    let payload = serde_json::json!({
        "business_param": {
            "aigc_template_id": "",
            "aigc_template_text": "",
            "book_id": series_id,
            "comment_tag_list": [],
            "from_famous_comment_id": 0,
            "has_aigc_content": false,
            "ignore_urge_rule": false,
            "is_confirm_request": false,
            "offset": 0,
            "read_item_cnt": 0,
            "score": score,
            "support_para_audio_play": false,
            "text_feature": {},
            "video_is_muted": 0,
        },
        "comment_type": COMMENT_TYPE_REVIEW_SEND,
        "commit_source": COMMIT_SOURCE_REVIEW,
        "data_type": DATA_TYPE_REVIEW,
        "group_id": series_id,
        "group_type": GROUP_TYPE_REVIEW,
        "image_data": [],
        "rich_text": [],
        "text": text,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造剧评请求失败: {e}")))?;
    let bytes = crate::client::api_call_reading(
        LQ_API_ORIGIN,
        crate::interact::COMMENT_ADD_PATH,
        Some(raw),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评响应失败: {e}")))?;
    check_comment_code(&value)?;
    // 响应是完整 comment_info（expand.score 回显评分"10"、头像昵称 uid
    // 齐全）——整体返回，前端直接置顶插入（2026-10-10 抓包对齐）
    let raw = value
        .pointer("/data/comment_info")
        .ok_or_else(|| AppError::Media("剧评发送响应缺少 comment_info".into()))?;
    parse_comment_item(raw).ok_or_else(|| AppError::Media("剧评发送响应对象字段不全".into()))
}

fn check_comment_code(v: &Value) -> AppResult<()> {
    if v.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!(
            "评论接口返回 {}: {msg}",
            v.get("code").and_then(Value::as_i64).unwrap_or(-1)
        )));
    }
    Ok(())
}

/// 单条评论对象（`data_list[].comment` 或发送响应的 `comment_info`，
/// 两者同构）→ [`CommentItem`]。id/文本缺失返回 None（列表侧跳过、
/// 发送侧报错由调用方处理）。
pub(crate) fn parse_comment_item(comment: &Value) -> Option<CommentItem> {
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
    // 附图评论（本地图片/表情）正文为空——有图就算一条评论（2026-10-25 抓包）
    let images = parse_comment_images(comment);
    if comment_id.is_empty() || (text.is_empty() && images.is_empty()) {
        return None;
    }
    let base = comment.pointer("/common/user_info/base_info");
    Some(CommentItem {
        comment_id,
        // 删除入口要对比登录 uid；user_id 在 user_info 层（base_info 兜底，
        // 匿名样本两层都可能缺，缺了前端就不显示删除钮）
        user_id: comment
            .pointer("/common/user_info/user_id")
            .or_else(|| comment.pointer("/common/user_info/base_info/user_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        user_name: base
            .and_then(|b| b.get("user_name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        avatar: base
            .and_then(|b| b.get("expand_user_avatar"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text,
        // 官方为 unix 秒；对外契约统一 UTC 毫秒
        create_time: comment
            .pointer("/common/create_timestamp")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 1000,
        digg_count: comment
            .pointer("/stat/digg_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        reply_count: comment
            .pointer("/stat/reply_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        user_digg: comment
            .pointer("/user_action/user_digg")
            .and_then(Value::as_bool)
            == Some(true),
        // 剧评评分（expand.score 十分制字符串 + 后缀文案）；单集评论无 expand.score 恒空。
        // 发送响应同样回显（2026-10-10 抓包：剧评发送后 comment_info.expand.score="10"）
        score: comment
            .pointer("/expand/score")
            .map(|x| match x {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        score_suffix_text: comment
            .pointer("/expand/score_suffix_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        images,
    })
}

/// 评论附图列表（`common.content.image_data_list.image_data[]`）。
///
/// 2026-10-25 抓包：官方回显的图与发送时的 `image_data` 同构，另带
/// `expand_web_url` / `web_url`（缩略图形态 `-image-v1:300:0.heic`）——
/// 渲染优先用 `web_uri`（原图），缩略图走 `thumb_nails`。
fn parse_comment_images(comment: &Value) -> Vec<crate::emoticon::Emoticon> {
    comment
        .pointer("/common/content/image_data_list/image_data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(crate::emoticon::parse_emoticon)
                .collect()
        })
        .unwrap_or_default()
}

/// 评论响应 → [`CommentPage`]（单集评论与剧级评论同构，共用）。
fn parse_comment_page(v: &Value) -> AppResult<CommentPage> {
    let list_info = v
        .pointer("/data/common_list_info")
        .cloned()
        .unwrap_or(Value::Null);
    let mut page = CommentPage {
        total: list_info.get("total").and_then(Value::as_i64).unwrap_or(0),
        has_more: list_info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: list_info
            .get("cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items: Vec::new(),
    };
    for entry in v
        .pointer("/data/data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(comment) = entry.get("comment") else {
            continue;
        };
        if let Some(item) = parse_comment_item(comment) {
            page.items.push(item);
        }
    }
    Ok(page)
}

/// 回复列表请求体——**单集评论维度**（2026-10-10 抓展开
/// 评论回复实操锁定）：路径 `/novel/commentapi/reply/list/{comment_id}/v1/`，
/// `group_id` 仍是分集 vid，`comment_source=504` / `comment_type=4` /
/// `server_channel=18`（回复拉取是独立于评论拉取的第三形态，别混用
/// comment/list 的 4/4/18 组合——source 必须是 504）；`cursor` 首页是
/// **显式空串**（与剧评维度「首屏无 cursor 键」不同），翻页回传
/// `comment_list_info.cursor`（数字串 "10"/"20"…）。
pub(super) fn comment_replies_payload(
    group_id: &str,
    book_id: &str,
    comment_id: &str,
    cursor: &str,
) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {"book_id": book_id, "need_count": false},
        "comment_id": comment_id,
        "comment_source": LIST_SOURCE_COMMENT_REPLIES,
        "comment_type": COMMENT_TYPE_COMMENT,
        "compliance_status": 0,
        "count": 10,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": GROUP_TYPE_COMMENT,
        "server_channel": SERVER_CHANNEL_COMMENT,
    })
}

/// 回复列表请求体——**剧评维度**（2026-10-10 抓展开/翻页
/// 剧评回复实操锁定，3 页全量）：`group_id` 是 series_id，
/// `comment_source=501` / `comment_type=2` / `server_channel=34`，
/// business_param 带 `real_level:2`；**首屏不带 cursor 键**（与 comment/list
/// 剧评形态同款纪律），翻页才回传；`need_count` 首页 true、翻页 false。
pub(super) fn review_replies_payload(series_id: &str, comment_id: &str, cursor: &str) -> Value {
    let mut payload = serde_json::json!({
        "business_param": {
            "book_id": series_id,
            "need_count": cursor.is_empty(),
            "real_level": 2,
        },
        "comment_id": comment_id,
        "comment_source": LIST_SOURCE_REVIEW_REPLIES,
        "comment_type": COMMENT_TYPE_REVIEW,
        "count": 10,
        "group_id": series_id,
        "group_type": GROUP_TYPE_REVIEW,
        "server_channel": SERVER_CHANNEL_REVIEW,
    });
    if !cursor.is_empty() {
        payload["cursor"] = Value::String(cursor.to_string());
    }
    payload
}

/// 拉一条**单集评论**的回复列表一页（`group_id`=vid、`book_id`=series_id；
/// `cursor` 传上一页返回的 `next_cursor`，首页传空）。
pub async fn fetch_comment_replies(
    group_id: &str,
    book_id: &str,
    comment_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<ReplyPage> {
    let path = format!("/novel/commentapi/reply/list/{comment_id}/v1/");
    let body = serde_json::to_vec(&comment_replies_payload(
        group_id, book_id, comment_id, cursor,
    ))
    .map_err(|e| AppError::Signer(e.to_string()))?;
    fetch_replies(&path, body, env).await
}

/// 拉一条**剧评**的回复列表一页（详情页剧评回复；参数同上）。
pub async fn fetch_review_replies(
    series_id: &str,
    comment_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<ReplyPage> {
    let path = format!("/novel/commentapi/reply/list/{comment_id}/v1/");
    let body = serde_json::to_vec(&review_replies_payload(series_id, comment_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    fetch_replies(&path, body, env).await
}

/// reply/list 共用入口：两维度同端点同响应结构，只差请求形态。
async fn fetch_replies(path: &str, body: Vec<u8>, env: &ApiEnv) -> AppResult<ReplyPage> {
    let bytes = crate::client::api_call_reading(LQ_API_ORIGIN, path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析回复列表响应失败: {e}")))?;
    check_comment_code(&v)?;
    parse_reply_page(&v)
}

/// 单条回复对象（reply/list 条目或 reply/add 发送响应的 `data.reply`，
/// 两者同构）→ [`ReplyItem`]。id/文本缺失返回 None。
pub(crate) fn parse_reply_item(reply: &Value) -> Option<ReplyItem> {
    let reply_id = reply
        .get("reply_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // Common 大写是实测形态；小写兜一层防上游将来统一
    let common = reply.get("Common").or_else(|| reply.get("common"));
    let text = common
        .and_then(|c| c.pointer("/content/text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if reply_id.is_empty() || text.is_empty() {
        return None;
    }
    let base = common.and_then(|c| c.pointer("/user_info/base_info"));
    Some(ReplyItem {
        reply_id,
        // 同 CommentItem：删除入口对比 uid 用（回复在两层都有，
        // user_info 层优先）
        user_id: common
            .and_then(|c| c.pointer("/user_info/user_id"))
            .or_else(|| common.and_then(|c| c.pointer("/user_info/base_info/user_id")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        user_name: base
            .and_then(|b| b.get("user_name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        avatar: base
            .and_then(|b| b.get("expand_user_avatar"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text,
        // 官方为 unix 秒；对外契约统一 UTC 毫秒
        create_time: common
            .and_then(|c| c.get("create_timestamp"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 1000,
        digg_count: reply
            .pointer("/stat/digg_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        user_digg: reply
            .pointer("/user_action/user_digg")
            .and_then(Value::as_bool)
            == Some(true),
        reply_to_name: reply
            .pointer("/reply_to_user_info/base_info/user_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        reply_to_reply_id: reply
            .get("reply_to_reply_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// reply/list 响应 → [`ReplyPage`]。
///
/// 结构与 comment/list 不同：条目在 `data.reply_list[]`（不是 data_list），
/// 分页在 `data.comment_list_info`（不是 common_list_info）；回复体在
/// 键名**大写**的 `Common` 下（上游序列化怪癖，照抓包兼容大小写两种）。
pub(super) fn parse_reply_page(v: &Value) -> AppResult<ReplyPage> {
    let info = v
        .pointer("/data/comment_list_info")
        .cloned()
        .unwrap_or(Value::Null);
    let mut page = ReplyPage {
        total: info.get("total").and_then(Value::as_i64).unwrap_or(0),
        has_more: info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: info
            .get("cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items: Vec::new(),
    };
    for reply in v
        .pointer("/data/reply_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(item) = parse_reply_item(reply) {
            page.items.push(item);
        }
    }
    Ok(page)
}

// ---------------------------------------------------------------- 创作者主页

/// 创作者主页资料头：`GET /reading/user/profile/get/v:version/?user_id=`，
/// 取 `data.ugc_user.base_info`（2026-10-15 官方实包：详情页点创作者 →
/// 主页，头部昵称/头像/简介）。
pub async fn fetch_creator_profile(user_id: &str, env: &ApiEnv) -> AppResult<CreatorProfile> {
    let q = [("user_id".to_string(), user_id.to_string())];
    let bytes = crate::client::api_call_reading(
        LQ_API_ORIGIN,
        "/reading/user/profile/get/v:version/",
        None,
        &q,
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析主页资料失败: {e}")))?;
    // reading 域错误可能走 BaseResp.StatusCode（无 code 键），两种都容
    if let Some(status) = v.pointer("/BaseResp/StatusCode").and_then(Value::as_i64)
        && status != 0
    {
        return Err(AppError::Media(format!("主页资料接口返回 {status}")));
    }
    check_comment_code(&v)?;
    let base = v
        .pointer("/data/ugc_user/base_info")
        .ok_or_else(|| AppError::Media("主页资料缺少 ugc_user.base_info".into()))?;
    Ok(CreatorProfile {
        user_id: base
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        user_name: base
            .get("user_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        avatar: base
            .get("user_avatar")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        description: base
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 创作者主页作品列表一页：`POST /novel/commentapi/user/profile/{uid}/v1/`。
/// 请求体照抄 2026-10-15 官方实包（作品 tab 形态 tab_type=23）；cursor 传
/// 上一页 `common_list_info.cursor` 原样串，首页空串。
pub async fn fetch_creator_works(
    user_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<CreatorWorksPage> {
    let payload = serde_json::json!({
        "aid": 8662,
        "comment_source": LIST_SOURCE_PROFILE_WORKS,
        "count": 18,
        "cursor": cursor,
        "extra_param": { "is_video_tab_comment": "1" },
        "locate_post_id": "",
        "profile_user_type": 0,
        "sort": "time_desc",
        "sub_tab_type": PROFILE_SUB_TAB_WORKS,
        "tab_type": PROFILE_TAB_WORKS,
        "target_user_id": user_id,
    });
    let path = format!("/novel/commentapi/user/profile/{user_id}/v1/");
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造主页作品请求失败: {e}")))?;
    let bytes = crate::client::api_call_reading(LQ_API_ORIGIN, &path, Some(raw), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析主页作品失败: {e}")))?;
    check_comment_code(&v)?;
    Ok(CreatorWorksPage {
        works: v
            .pointer("/data/data_list")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(parse_creator_work)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        has_more: v
            .pointer("/data/common_list_info/has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cursor: v
            .pointer("/data/common_list_info/cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        total: v
            .pointer("/data/hightlight/total")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn parse_creator_work(raw: &Value) -> Option<CreatorWork> {
    // series_id（video_id）是播放目标的唯一钥匙，缺了这条就没法跳
    let series_id = raw
        .pointer("/video/video_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    Some(CreatorWork {
        series_id: series_id.to_string(),
        title: raw
            .pointer("/video/common/title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover: raw
            .pointer("/video/common/cover")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        intro: raw
            .pointer("/video/common/intro")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        episode_cnt: raw
            .pointer("/video/expand/episode_cnt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        play_cnt: raw
            .pointer("/video/stat/play_cnt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        hot: raw
            .pointer("/video/stat/hot")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        tags: raw
            .pointer("/video/expand/sub_title_list")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("text").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod profile_probe {
    use super::*;
    use crate::client::ApiEnv;
    use hongguo_common::model::ProxyConfig;

    /// 活体探测：主页资料 + 作品列表全链。uid 463328455691016 = 墨子动漫
    /// （官方抓包里点过出品方 chip 实锤 7 部作品）。
    /// 用法：`cargo test -p hongguo-api fetch_creator -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_creator_homepage_chain() {
        let env = ApiEnv::anonymous(ProxyConfig::default());
        let uid = std::env::var("PROBE_UID").unwrap_or_else(|_| "463328455691016".into());

        let profile = fetch_creator_profile(&uid, &env).await.expect("profile");
        println!(
            "[profile] {} avatar={}",
            profile.user_name,
            !profile.avatar.is_empty()
        );

        let page = fetch_creator_works(&uid, "", &env).await.expect("works");
        println!(
            "[works] total={} has_more={} cursor_len={}",
            page.total,
            page.has_more,
            page.cursor.len()
        );
        for w in &page.works {
            println!(
                "  《{}》{}集 播放{} tags={:?} sid={}",
                w.title, w.episode_cnt, w.play_cnt, w.tags, w.series_id
            );
        }
        if page.has_more && !page.cursor.is_empty() {
            let next = fetch_creator_works(&uid, &page.cursor, &env)
                .await
                .expect("works#2");
            println!("[works#2] {} 条", next.works.len());
        }
    }
}
