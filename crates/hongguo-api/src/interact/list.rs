//! 提及候选 / 追更列表 / 书架 / 互动状态。

use serde_json::Value;

use super::model::{
    BookshelfEntry, FollowUpdateItem, FollowUpdatePage, InteractionItem, InteractionState,
};
use crate::client::ApiEnv;
use crate::danmaku::LQ_API_ORIGIN;
use crate::params::interaction::ACTION_CHANNEL_FOLLOW;
use hongguo_common::{AppError, AppResult};

const BOOKSHELF_LIST_PATH: &str = "/reading/bookapi/bookshelf/video/list/v";
const UGC_MGET_PATH: &str = "/reading/ugc/action/mget/v";
const FOLLOW_LIST_PATH: &str = "/novel/articleapi/relation/get_user_action/v1/";

/// 拉追更列表一页（get_user_action `action_channel=9`）：`cursor` 传上页响应的
/// `cursor` 原文，首页传空串。「是否已追」没有独立接口——拉第一页本地判成员。
pub async fn fetch_follow_updates(cursor: &str, env: &ApiEnv) -> AppResult<FollowUpdatePage> {
    let biz_query: Vec<(String, String)> = vec![
        ("action_channel".into(), ACTION_CHANNEL_FOLLOW.to_string()),
        ("count".into(), "20".into()),
        ("cursor_str".into(), cursor.to_string()),
    ];
    let bytes =
        crate::client::api_call_reading(LQ_API_ORIGIN, FOLLOW_LIST_PATH, None, &biz_query, env)
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
pub(super) fn parse_follow_updates(value: &Value) -> FollowUpdatePage {
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

/// 拉账号的书架（收藏）列表，「我的收藏」页数据源；`target_user_id` 是
/// 登录用户 uid。响应条目字段名未在非空样本中验证过，按同族字段容错解析。
pub async fn fetch_bookshelf(target_user_id: &str, env: &ApiEnv) -> AppResult<Vec<BookshelfEntry>> {
    fetch_bookshelf_inner(target_user_id, false, env).await
}

/// Only this complete, well-formed shelf snapshot can prove that a favorite disappeared.
pub async fn fetch_bookshelf_strict(
    target_user_id: &str,
    env: &ApiEnv,
) -> AppResult<Vec<BookshelfEntry>> {
    fetch_bookshelf_inner(target_user_id, true, env).await
}

async fn fetch_bookshelf_inner(
    target_user_id: &str,
    strict: bool,
    env: &ApiEnv,
) -> AppResult<Vec<BookshelfEntry>> {
    let biz_query: Vec<(String, String)> =
        vec![("target_user_id".into(), target_user_id.to_string())];
    let bytes =
        crate::client::api_call_reading(LQ_API_ORIGIN, BOOKSHELF_LIST_PATH, None, &biz_query, env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析书架列表失败: {e}")))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        let msg = value.get("message").and_then(Value::as_str).unwrap_or("?");
        return Err(AppError::Media(format!("书架列表接口返回: {msg}")));
    }
    parse_bookshelf(&value, strict)
}

fn parse_bookshelf(value: &Value, strict: bool) -> AppResult<Vec<BookshelfEntry>> {
    if strict && value.pointer("/data/has_more").and_then(Value::as_bool) == Some(true) {
        return Err(AppError::Media("收藏列表不完整".into()));
    }
    let rows = value
        .pointer("/data/video_shelf_info")
        .and_then(Value::as_array);
    if strict && rows.is_none() {
        return Err(AppError::Media("收藏列表缺少条目数组".into()));
    }
    let rows = rows.map(Vec::as_slice).unwrap_or_default();
    let mut items = Vec::with_capacity(rows.len());
    for raw in rows {
        let Some(series_id) = ["series_id", "book_id", "item_id"]
            .into_iter()
            .filter_map(|key| raw.get(key))
            .find_map(bookshelf_id)
        else {
            if strict {
                return Err(AppError::Media("收藏列表包含无法识别的条目".into()));
            }
            continue;
        };
        let book_id = raw
            .get("book_id")
            .and_then(bookshelf_id)
            .unwrap_or_else(|| series_id.clone());
        let book_type = raw
            .get("book_type")
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
            .filter(|&value| value > 0);
        // 候选字段秒/毫秒形态并存，按值判量级（1e12 ≈ 2001 年起的毫秒戳）：
        // 毫秒原样用，秒形态 ×1000。
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
            book_id,
            book_type,
            collect_time_ms,
            content_type,
        });
    }
    Ok(items)
}

/// 排除空占位值，再按字段顺序尝试；JSON 整数保留 u64 全精度。
pub(super) fn bookshelf_id(value: &Value) -> Option<String> {
    if let Some(id) = value.as_str() {
        let id = id.trim();
        return (!id.is_empty() && id != "0").then(|| id.to_owned());
    }
    value
        .as_u64()
        .filter(|&id| id != 0)
        .map(|id| id.to_string())
}

/// 拉互动状态列表（`ugc/action/mget`，最近 100 条）——列表接口而非单集查询：
/// 前端用当前 vid / series_id 匹配，不在列表里就当未互动（best-effort）。
pub async fn fetch_interaction_state(env: &ApiEnv) -> AppResult<InteractionState> {
    let biz_query: Vec<(String, String)> = vec![
        ("action_type".into(), "3".into()),
        ("count".into(), "100".into()),
        ("offset".into(), "0".into()),
        ("object_type_list".into(), "6,15,10".into()),
    ];
    let bytes =
        crate::client::api_call_reading(LQ_API_ORIGIN, UGC_MGET_PATH, None, &biz_query, env)
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
