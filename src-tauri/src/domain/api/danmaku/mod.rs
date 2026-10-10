//! 弹幕：commentapi 的 comment/list（comment_type=20）。
//!
//! 实测口径（2026-10 抓包锁定）：
//! - `POST {LQ_API_ORIGIN}/novel/commentapi/comment/list/{group_id}/v1/`，
//!   **group_id（=分集 vid）在路径里**；
//! - 参数在 body **顶层**（不包 biz_param），抓包形状见 [`danmaku_payload`]；
//! - 自定义头 `x-reading-request: {ticket_ms}-{random}`；
//! - 分页靠 `cursor`（`{"start_offset_time":ms,"end_offset_time":ms}` 30 秒窗口），
//!   一集要多拉几窗才能拿全，本模块代为循环（上限见 [`MAX_WINDOWS`]）；
//! - 弹幕的**时间轴**在 `comment.expand.offset_time`（毫秒，与 cursor 同单位）。
//!
//! 发送（comment/add）需要登录态，登录里程碑落地后再接。

mod model;

pub use model::{
    CommentItem, CommentPage, CommentTagStat, Danmaku, ReplyItem, ReplyPage, SeriesReviewPage,
};

use serde_json::Value;

use super::client::ApiEnv;
use crate::error::{AppError, AppResult};

/// hgplayer 实测的 reading 系 API 主机（我们的视频接口走 sinfonlineb，
/// commentapi 只在这个主机上存在）。
pub const LQ_API_ORIGIN: &str = "https://api5-normal-lq.fqnovel.com";

/// 一集最多拉多少个 30 秒窗口（2 小时封顶，防服务端 cursor 异常死循环）。
const MAX_WINDOWS: usize = 240;

/// 拉一集的**全部**弹幕（内部按 cursor 窗口循环到 has_more=false）。
///
/// `group_id` 是分集 vid，`book_id` 是 series_id——两个都来自剧集档案。
pub async fn fetch_danmaku_all(
    group_id: &str,
    book_id: &str,
    env: &ApiEnv,
) -> AppResult<Vec<Danmaku>> {
    let mut all: Vec<Danmaku> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = String::new();
    for _ in 0..MAX_WINDOWS {
        let page = fetch_danmaku_window(group_id, book_id, &cursor, env).await?;
        let has_more = page.has_more;
        cursor = page.next_cursor;
        for d in page.items {
            if seen.insert(d.comment_id.clone()) {
                all.push(d);
            }
        }
        if !has_more {
            break;
        }
    }
    all.sort_by_key(|d| d.offset_ms);
    Ok(all)
}

/// 一窗弹幕。
struct DanmakuWindow {
    items: Vec<Danmaku>,
    has_more: bool,
    next_cursor: String,
}

/// 抓包形状的请求体。
fn danmaku_payload(group_id: &str, book_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_danmaku_guide_type": [],
            "playlet_item_duration": 60000,
            "start_offset_time": 0,
        },
        "comment_source": 601,
        "comment_type": 20,
        "compliance_status": 0,
        "count": 90,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 30,
        "server_channel": 1000,
        "sort": 1,
    })
}

/// 评论区请求体（与弹幕同端点不同形态）。
///
/// 2026-10-06 抓 hgplayer 1.1.5 重锁：评论区形态与弹幕形态在服务端是
/// **两套校验**——沿用 1.1.3 的弹幕形 business_param + server_channel=1000
/// 会被拒（实测 103001 参数错误）；评论必须用 `need_count/req_type` +
/// `server_channel=18`，且 `need_count: true` 让响应带上评论总数
/// （`common_list_info.total`，互动栏评论计数的数据源）。
fn comments_payload(group_id: &str, book_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {
            "book_id": book_id,
            "need_count": true,
            "req_type": 0,
        },
        "comment_source": 4,
        "comment_type": 4,
        "compliance_status": 0,
        "count": 20,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 30,
        "server_channel": 18,
        "sort": 1,
    })
}

async fn fetch_danmaku_window(
    group_id: &str,
    book_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<DanmakuWindow> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&danmaku_payload(group_id, book_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    // reading 系统一入口：轻签名头 + gzip body（1.1.3 抓包形态）
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析弹幕响应失败: {e}")))?;
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
    Ok(DanmakuWindow {
        has_more: list_info
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: list_info
            .get("cursor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        items,
    })
}

/// 拉评论区的一页（cursor 传上一页返回的 next_cursor，首页传空）。
pub async fn fetch_comments_page(
    group_id: &str,
    book_id: &str,
    cursor: &str,
    env: &ApiEnv,
) -> AppResult<CommentPage> {
    let path = format!("/novel/commentapi/comment/list/{group_id}/v1/");
    let body = serde_json::to_vec(&comments_payload(group_id, book_id, cursor))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析评论响应失败: {e}")))?;
    check_comment_code(&v)?;
    parse_comment_page(&v)
}

/// 剧级评论（详情页「剧评」，整部剧一条线）。
///
/// 2026-10-07 抓 hgplayer 详情页锁定：同一端点换**形态**——`group_id` 是
/// **series_id**（不是 vid）、`group_type=1`（书/剧维度）、`comment_source=1`、
/// `comment_type=2`、`server_channel=34`；business_param 是另一组全量字段
/// （照抄抓包，审计纪律）。响应与单集评论同构（common_list_info.total /
/// data_list），解析复用。
fn series_comments_payload(group_id: &str, cursor: &str) -> Value {
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
        "comment_source": 1,
        "comment_type": 2,
        "count": 10,
        "group_id": group_id,
        "group_type": 1,
        "server_channel": 34,
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
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, &path, Some(body), &[], env).await?;
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
/// 参数照 hgplayer 1.1.6 抓包逐字段对齐（2026-10-10，用户实测发满分
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
) -> AppResult<String> {
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
        "comment_type": 0,
        "commit_source": 12,
        "data_type": 2,
        "group_id": series_id,
        "group_type": 1,
        "image_data": [],
        "rich_text": [],
        "text": text,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造剧评请求失败: {e}")))?;
    let bytes = super::client::api_call_reading(
        LQ_API_ORIGIN,
        super::interact::COMMENT_ADD_PATH,
        Some(raw),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评响应失败: {e}")))?;
    check_comment_code(&value)?;
    Ok(value
        .pointer("/data/comment_info/comment_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
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
        let base = comment.pointer("/common/user_info/base_info");
        page.items.push(CommentItem {
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
            create_time: comment
                .pointer("/common/create_timestamp")
                .and_then(Value::as_i64)
                .unwrap_or(0),
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
            // 剧评评分（expand.score 十分制字符串 + 后缀文案）；单集评论无 expand.score 恒空
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
        });
    }
    Ok(page)
}

/// 回复列表请求体——**单集评论维度**（2026-10-10 抓 hgplayer 1.1.8 展开
/// 评论回复实操锁定）：路径 `/novel/commentapi/reply/list/{comment_id}/v1/`，
/// `group_id` 仍是分集 vid，`comment_source=504` / `comment_type=4` /
/// `server_channel=18`（回复拉取是独立于评论拉取的第三形态，别混用
/// comment/list 的 4/4/18 组合——source 必须是 504）；`cursor` 首页是
/// **显式空串**（与剧评维度「首屏无 cursor 键」不同），翻页回传
/// `comment_list_info.cursor`（数字串 "10"/"20"…）。
fn comment_replies_payload(group_id: &str, book_id: &str, comment_id: &str, cursor: &str) -> Value {
    serde_json::json!({
        "aid": 8662,
        "business_param": {"book_id": book_id, "need_count": false},
        "comment_id": comment_id,
        "comment_source": 504,
        "comment_type": 4,
        "compliance_status": 0,
        "count": 10,
        "cursor": cursor,
        "group_id": group_id,
        "group_type": 30,
        "server_channel": 18,
    })
}

/// 回复列表请求体——**剧评维度**（2026-10-10 抓 hgplayer 1.1.8 展开/翻页
/// 剧评回复实操锁定，3 页全量）：`group_id` 是 series_id，
/// `comment_source=501` / `comment_type=2` / `server_channel=34`，
/// business_param 带 `real_level:2`；**首屏不带 cursor 键**（与 comment/list
/// 剧评形态同款纪律），翻页才回传；`need_count` 首页 true、翻页 false。
fn review_replies_payload(series_id: &str, comment_id: &str, cursor: &str) -> Value {
    let mut payload = serde_json::json!({
        "business_param": {
            "book_id": series_id,
            "need_count": cursor.is_empty(),
            "real_level": 2,
        },
        "comment_id": comment_id,
        "comment_source": 501,
        "comment_type": 2,
        "count": 10,
        "group_id": series_id,
        "group_type": 1,
        "server_channel": 34,
    });
    if !cursor.is_empty() {
        payload["cursor"] = Value::String(cursor.to_string());
    }
    payload
}

/// 拉一条**单集评论**的回复列表一页（`cursor` 传上一页返回的
/// `next_cursor`，首页传空）。
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
    let bytes = super::client::api_call_reading(LQ_API_ORIGIN, path, Some(body), &[], env).await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析回复列表响应失败: {e}")))?;
    check_comment_code(&v)?;
    parse_reply_page(&v)
}

/// reply/list 响应 → [`ReplyPage`]。
///
/// 结构与 comment/list 不同：条目在 `data.reply_list[]`（不是 data_list），
/// 分页在 `data.comment_list_info`（不是 common_list_info）；回复体在
/// 键名**大写**的 `Common` 下（上游序列化怪癖，照抓包兼容大小写两种）。
fn parse_reply_page(v: &Value) -> AppResult<ReplyPage> {
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
            continue;
        }
        let base = common.and_then(|c| c.pointer("/user_info/base_info"));
        page.items.push(ReplyItem {
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
            create_time: common
                .and_then(|c| c.get("create_timestamp"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
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
        });
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 剧评发送形态抓包锚点（known-issues 后续对齐，2026-10-10 hgplayer
    /// 1.1.6 实测发满分剧评）：data_type=2 / commit_source=12 /
    /// comment_type=0，评分在 business_param.score（十分制）。任何改形
    /// 都会被服务端 103008「无社区功能」拒收。
    #[test]
    fn review_send_payload_anchors() {
        let src = include_str!("mod.rs");
        for (needle, why) in [
            (
                r#""commit_source": 12"#,
                "剧评发送 commit_source=12（评论 3 / 弹幕 1500 / 回复 9 都不对）",
            ),
            (r#""data_type": 2,"#, "剧评 data_type=2（评论 4 / 弹幕 20）"),
            (r#""comment_type": 0,"#, "剧评 comment_type=0"),
            (
                r#""score": score,"#,
                "评分在 business_param.score（十分制，5 星 ×2）",
            ),
        ] {
            assert!(src.contains(needle), "{}：缺少锚点 {}", why, needle);
        }
    }

    #[test]
    fn payload_shape_matches_capture() {
        let p = danmaku_payload("7690197301075119166", "7690150906532219966", "");
        // 抓包锚点字段：任何一个改形都会 100103
        assert_eq!(p["comment_type"], 20, "20 = 弹幕");
        assert_eq!(p["comment_source"], 601);
        assert_eq!(p["group_type"], 30);
        assert_eq!(p["count"], 90);
        assert_eq!(p["aid"], 8662);
        assert_eq!(p["group_id"], "7690197301075119166");
        assert_eq!(p["business_param"]["book_id"], "7690150906532219966");
        assert_eq!(p["cursor"], "");
    }

    #[test]
    fn window_cursor_passthrough() {
        let p = danmaku_payload(
            "g",
            "b",
            r#"{"start_offset_time":30000,"end_offset_time":60000}"#,
        );
        assert!(p["cursor"].as_str().unwrap().contains("end_offset_time"));
    }

    #[test]
    fn comments_payload_matches_capture_20261006() {
        // 2026-10-06 抓 hgplayer 1.1.5：评论形态与弹幕形态分叉，
        // 沿用弹幕形 business_param / server_channel=1000 会被 103001 拒
        let p = comments_payload("g", "b", "");
        assert_eq!(p["comment_type"], 4);
        assert_eq!(p["comment_source"], 4);
        assert_eq!(p["server_channel"], 18, "评论走 18，弹幕才是 1000");
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["business_param"]["req_type"], 0);
        assert_eq!(p["count"], 20);
        assert!(
            p["business_param"].get("playlet_item_duration").is_none(),
            "弹幕形字段混进评论体会 103001"
        );
    }

    #[test]
    fn series_comments_payload_matches_capture_20261007() {
        // 2026-10-07 抓 hgplayer 详情页：剧级评论换形态——group_id 是
        // series_id、group_type=1、source=1/type=2/channel=34，
        // business_param 是全量字段组（照抄抓包）
        let p = series_comments_payload("7687941387362257982", "");
        assert_eq!(p["group_type"], 1, "1 = 书/剧维度，30 是单集");
        assert_eq!(p["comment_source"], 1);
        assert_eq!(p["comment_type"], 2);
        assert_eq!(p["server_channel"], 34);
        assert_eq!(p["group_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["book_id"], "7687941387362257982");
        assert_eq!(p["business_param"]["need_count"], true);
        assert_eq!(p["count"], 10);
    }

    #[test]
    fn comment_replies_payload_matches_capture_20261010() {
        // 2026-10-10 抓 hgplayer 1.1.8 展开评论回复：source=504（评论拉取
        // 是 4）+ 首页显式空 cursor + compliance_status 在场——三处都与
        // comment/list 形态不同，混用会被拒
        let p = comment_replies_payload("g", "b", "c", "");
        assert_eq!(p["comment_source"], 504, "回复拉取走 504");
        assert_eq!(p["comment_type"], 4);
        assert_eq!(p["server_channel"], 18);
        assert_eq!(p["group_type"], 30);
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
        // 2026-10-10 抓 hgplayer 1.1.8 展开/翻页剧评回复（3 页全量）：
        // source=501/type=2/ch=34 + real_level=2 + 首屏无 cursor 键 +
        // need_count 首页 true 翻页 false
        let p = review_replies_payload("s", "c", "");
        assert_eq!(p["comment_source"], 501, "剧评回复拉取走 501");
        assert_eq!(p["comment_type"], 2);
        assert_eq!(p["server_channel"], 34);
        assert_eq!(p["group_type"], 1);
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
        // 回复条目在键名大写的 Common 下（上游序列化怪癖，抓包实锤）；
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

    /// 拉一集的**全部**评论区（按 cursor 翻页到 has_more=false，上限见 [`MAX_WINDOWS`]）。
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
    async fn probe_danmaku_full() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
        let all = fetch_danmaku_all("7690197301075119166", "7690150906532219966", &env)
            .await
            .expect("整集弹幕");
        println!(
            "[danmaku-full] {} 条，前 5: {:?}",
            all.len(),
            &all[..all.len().min(5)]
        );
        assert!(!all.is_empty(), "这一集实测有弹幕");
        // 时间轴升序（fetch_danmaku_all 已排序）
        let mut sorted = all.clone();
        sorted.sort_by_key(|d| d.offset_ms);
        assert_eq!(all, sorted);
    }

    /// 评论区（ct=4/src=4）匿名可读性探测：2026-10-06 实录旧弹幕形态被拒
    /// （103001），换 1.1.5 新形态（need_count/req_type/ch=18）后应恢复。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_comments_full() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
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

    /// 回复列表双维度探测（2026-10-10 抓 hgplayer 1.1.8 锁定形态后落地）。
    /// 锚点取自当轮抓包：剧评 7693600825641861912（book=7691228619774905368）
    /// 与评论 7693242834317837081（vid=7691249364097829913）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例（匿名可读，无需登录态）"]
    async fn probe_reply_lists() {
        let env = ApiEnv::anonymous(crate::domain::model::ProxyConfig::default());
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
