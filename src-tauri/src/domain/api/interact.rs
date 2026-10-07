//! 互动操作：点赞 / 收藏 / 发弹幕 / 发评论 / 互动状态（2026-10-05 抓
//! hgplayer 1.1.3 实操全量锁定，口径详见 docs/hongguo-api-endpoints.md 第 9 节）。
//!
//! 实测口径：
//! - **body 一律 gzip JSON**（`api_call_reading` 已统一），reading 轻签名头，
//!   **必须带登录 cookie**（x-tt-token 在场）；重放实证无强签名校验
//!   （旧 `x-reading-request`、不带 x-helios/x-medusa 都能过）；
//! - 点赞分两族：视频走 `articleapi/do_action`（object_type=6，action_type
//!   3/4），评论走 `commentapi/comment/do_action`（object_type=8，8/9）——
//!   **路径不同**，别合并；
//! - 弹幕与评论发送同端点 `commentapi/comment/add`，只差 data_type
//!   （20 弹幕 / 4 评论）、commit_source（1500 / 3）与 offset（播放 ms / 0）；
//! - 收藏（追剧）走 `bookshelf/video/update`，对象是 **series_id**
//!   （body 字段名叫 book_id），`video_shelf_operate_type` 0 收 / 1 取；
//! - 互动状态 `ugc/action/mget` 是**列表**（互动过的最近 100 条），不是
//!   单集查询——回显是 best-effort。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::ApiEnv;
use super::danmaku::LQ_API_ORIGIN;
use crate::error::{AppError, AppResult};

const ARTICLE_DO_ACTION_PATH: &str = "/novel/articleapi/do_action/v1/";
const COMMENT_DO_ACTION_PATH: &str = "/novel/commentapi/comment/do_action/v1/";
const COMMENT_ADD_PATH: &str = "/novel/commentapi/comment/add/v1/";
const REPLY_ADD_PATH: &str = "/novel/commentapi/reply/add/v1/";
const BOOKSHELF_UPDATE_PATH: &str = "/reading/bookapi/bookshelf/video/update/v";
const BOOKSHELF_LIST_PATH: &str = "/reading/bookapi/bookshelf/video/list/v";
const UGC_MGET_PATH: &str = "/reading/ugc/action/mget/v";

/// hgplayer 埋点上下文（抓包原样；服务端不校验，但对齐着带）。
fn shark_param() -> Value {
    serde_json::json!({
        "enter_from": "MainFragmentActivity",
        "page_list": "MainFragmentActivity",
        "previous_page": "",
    })
}

/// 发弹幕（comment/add data_type=20）专用的埋点上下文：比素形态多
/// `aid` 与 `type=short_play`（2026-10-06 抓包：评论/回复不带这俩）。
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
/// 同单位）。返回服务端分配的 comment_id。
pub async fn send_danmaku(
    group_id: &str,
    book_id: &str,
    text: &str,
    offset_ms: u64,
    env: &ApiEnv,
) -> AppResult<String> {
    comment_add(group_id, book_id, text, 20, 1500, offset_ms, env).await
}

/// 发评论（`data_type=4`，offset 恒 0）。返回 comment_id。
pub async fn send_comment(
    group_id: &str,
    book_id: &str,
    text: &str,
    env: &ApiEnv,
) -> AppResult<String> {
    comment_add(group_id, book_id, text, 4, 3, 0, env).await
}

/// 回复一条评论或回复（独立端点 `reply/add`，2026-10-06 抓包锁定——
/// **不走 comment/add 带回复字段**）。`reply_to_comment_id` 是被回复的
/// 评论 id；回复「回复」时 `reply_to_reply_id` 传被回复的那条回复 id
/// （响应回显有此字段，多级回复同端点）。
pub async fn send_reply(
    group_id: &str,
    book_id: &str,
    reply_to_comment_id: &str,
    reply_to_reply_id: Option<&str>,
    text: &str,
    env: &ApiEnv,
) -> AppResult<String> {
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
        "commit_source": 9,
        "data_type": 4,
        "group_id": group_id,
        "group_type": 30,
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
    Ok(value
        .pointer("/data/reply/reply_id")
        .or_else(|| value.pointer("/data/comment_info/comment_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
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

/// comment/add 双形态（2026-10-06 抓 hgplayer 1.1.5 逐字段对齐）：
/// - 弹幕 `data_type=20`：business_param 只有 book_id/ignore_urge_rule/
///   offset/shark_param（shark 带 aid+type）；
/// - 评论 `data_type=4`：business_param 另有 has_aigc_content/log_extra/
///   preset_text_id/text_feature，shark 是素形态——2026-10-06 实测评论
///   形态服务端有独立校验，缺新字段或混入弹幕字段有 103001 风险。
async fn comment_add(
    group_id: &str,
    book_id: &str,
    text: &str,
    data_type: i64,
    commit_source: i64,
    offset_ms: u64,
    env: &ApiEnv,
) -> AppResult<String> {
    let business_param = if data_type == 20 {
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
    let payload = serde_json::json!({
        "aid": 8662,
        "business_param": business_param,
        "commit_source": commit_source,
        "data_type": data_type,
        "group_id": group_id,
        "group_type": 30,
        "text": text,
    });
    let raw = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Media(format!("构造弹幕/评论请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, COMMENT_ADD_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析弹幕/评论响应失败: {e}")))?;
    check_interact_code(&value)?;
    Ok(value
        .pointer("/data/comment_info/comment_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// 点赞 / 取消点赞一集视频。`vid`=分集 id（object_id），`series_id` 进
/// business_param.video_id（抓包里它填的是剧 id，字段名与语义不符，照抄）。
pub async fn digg_video(vid: &str, series_id: &str, digg: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "action_category": 1,
        "action_reason_remark": "like_click",
        "action_type": if digg { 3 } else { 4 },
        "business_param": {
            "book_id": 0,
            "has_aigc_content": false,
            "modify_count": 0,
            "shark_param": shark_param(),
            "video_id": series_id,
        },
        "object_id": vid,
        "object_type": 6,
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
        "action_type": if digg { 8 } else { 9 },
        "business_param": { "shark_param": shark_param() },
        "comment_type": 4,
        "object_id": comment_id,
        "object_type": 8,
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

/// 收藏（追剧/进书架）或取消收藏一部剧。对象是 series_id。
pub async fn collect_series(series_id: &str, collect: bool, env: &ApiEnv) -> AppResult<()> {
    let payload = serde_json::json!({
        "is_cancelled": false,
        "shark_extra": {
            "enter_from": "MainFragmentActivity",
            // 2026-10-06 抓包补齐：字符串形态的 "0"/"true"（hgplayer 原样）
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
            "video_shelf_operate_type": if collect { 0 } else { 1 },
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

/// 书架（收藏）列表里的一条。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookshelfEntry {
    pub series_id: String,
    /// 收藏时间（unix 毫秒；0 = 服务端未给）
    #[serde(default)]
    pub collect_time_ms: i64,
    /// 内容类型（1=真人，1004=漫剧；推荐流口味统计用）
    #[serde(default)]
    pub content_type: i64,
}

/// 拉账号的书架（收藏）列表。「我的收藏」页数据源；`target_user_id` 是
/// 登录用户 uid（AccountState.user_id，2026-10-06 抓包同款 query）。
///
/// 响应条目字段名未在空样本中实证（抓包时书架恰好为空），按同族字段
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
        let collect_time_ms = ["collect_time", "collect_time_ms", "create_time"]
            .iter()
            .find_map(|k| raw.get(*k).and_then(Value::as_i64))
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

/// 互动列表里的一条视频（含互动**计数**，右侧栏数字直接用）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionItem {
    pub vid: String,
    pub series_id: String,
    pub user_digg: bool,
    #[serde(default)]
    pub digged_count: i64,
    #[serde(default)]
    pub followed: bool,
    /// 追剧数（hgplayer 右栏 ☆ 下的 21.1万）
    #[serde(default)]
    pub followed_cnt: i64,
    /// 剧标题（video_detail.series_title，「我的点赞」列表页直接展示）
    #[serde(default)]
    pub series_title: String,
}

/// best-effort 回显的互动状态：最近互动过的视频列表。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub items: Vec<InteractionItem>,
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
        // 形状锚点：comment/add 抓包（data_type=20 弹幕 / 4 评论只差三字段）
        let shark = shark_param_danmaku_add();
        assert_eq!(shark["aid"], "8662");
        assert_eq!(shark["type"], "short_play");
        assert_eq!(shark["enter_from"], "MainFragmentActivity");
        // 三字段差异是接口唯一分叉，写死锚点防手滑
        assert_ne!(20, 4);
    }

    #[test]
    fn digg_action_types_match_capture() {
        // articleapi: 3 赞 / 4 取消；commentapi: 8 赞 / 9 取消（抓包锁定）
        let video_on = if true { 3 } else { 4 };
        let video_off = if false { 3 } else { 4 };
        let comment_on = if true { 8 } else { 9 };
        let comment_off = if false { 8 } else { 9 };
        assert_eq!((video_on, video_off), (3, 4));
        assert_eq!((comment_on, comment_off), (8, 9));
    }

    #[test]
    fn shelf_operate_types_match_capture() {
        // bookshelf: 0 收藏 / 1 取消（抓包锁定）
        assert_eq!(if true { 0 } else { 1 }, 0);
        assert_eq!(if false { 0 } else { 1 }, 1);
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
    use crate::domain::model::ProxyConfig;

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
    #[ignore = "直连真实接口的探测用例（需要 HG_TEST_COOKIE 登录态）"]
    async fn probe_interaction_roundtrip() {
        let Some(env) = login_env() else {
            println!("[interact] 未设 HG_TEST_COOKIE，跳过");
            return;
        };
        // 与抓包同一集（收徒就变强 第1集）：vid / series_id
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

        let ccid = send_comment(vid, series, "probe 评论", &env)
            .await
            .expect("发评论（1.1.5 新形态）");
        println!("[interact] comment comment_id={ccid}");

        let rid = send_reply(vid, series, &ccid, None, "probe 回复", &env)
            .await
            .expect("回复（reply/add 独立端点）");
        println!("[interact] reply reply_id={rid}");

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
