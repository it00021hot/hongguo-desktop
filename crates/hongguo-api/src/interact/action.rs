//! 点赞 / 收藏 / 追更。

use serde_json::Value;

use super::{check_interact_code, shark_param};
use crate::client::ApiEnv;
use crate::danmaku::LQ_API_ORIGIN;
use crate::params::interaction::{
    ACTION_TYPE_FOLLOW, ACTION_TYPE_UNFOLLOW, COMMENT_TYPE_COMMENT, COMMENT_TYPE_REVIEW,
    OBJECT_TYPE_COMMENT, OBJECT_TYPE_REVIEW, OBJECT_TYPE_SERIES, OBJECT_TYPE_VIDEO,
};
use hongguo_common::{AppError, AppResult};

const ARTICLE_DO_ACTION_PATH: &str = "/novel/articleapi/do_action/v1/";
const COMMENT_DO_ACTION_PATH: &str = "/novel/commentapi/comment/do_action/v1/";
const BOOKSHELF_UPDATE_PATH: &str = "/reading/bookapi/bookshelf/video/update/v";


pub(super) fn video_digg_action_type(digg: bool) -> i64 {
    if digg { 3 } else { 4 }
}

pub(super) fn comment_digg_action_type(digg: bool) -> i64 {
    if digg { 8 } else { 9 }
}

pub(super) fn shelf_operate_type(collect: bool) -> i64 {
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
        crate::client::api_call_reading(LQ_API_ORIGIN, ARTICLE_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

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
        crate::client::api_call_reading(LQ_API_ORIGIN, COMMENT_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析评论点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 点赞 / 取消点赞一条**剧评**：与评论点赞（[`digg_comment`]）同路径但
/// `object_type`/`comment_type` 不同且 `business_param` 是空对象，别合并。
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
        crate::client::api_call_reading(LQ_API_ORIGIN, COMMENT_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析剧评点赞响应失败: {e}")))?;
    check_interact_code(&value)
}

pub async fn collect_series(series_id: &str, collect: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "is_cancelled": false,
        "shark_extra": {
            "enter_from": "MainFragmentActivity",
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
        crate::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_UPDATE_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析收藏响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 批量收藏/取消收藏：同一 `update_bookshelf_video_list` 携带多条目，
/// 一请求完成批量；`(series_id, collect)` 对，信封与单条完全同形。
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
        crate::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_UPDATE_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量收藏响应失败: {e}")))?;
    check_interact_code(&value)
}

/// 追更 / 取消追更一部剧：与点赞同端点（articleapi/do_action）但形态不同，
/// `object_type=10`、`object_id`=series_id、business_param **不带 video_id**，别合并。
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
        crate::client::api_call_reading(LQ_API_ORIGIN, ARTICLE_DO_ACTION_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析追更响应失败: {e}")))?;
    check_interact_code(&value)
}

pub(super) fn follow_action_type(follow: bool) -> i64 {
    if follow {
        ACTION_TYPE_FOLLOW
    } else {
        ACTION_TYPE_UNFOLLOW
    }
}
