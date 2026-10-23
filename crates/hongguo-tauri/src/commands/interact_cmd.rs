//! 互动 command：点赞 / 收藏 / 发弹幕 / 发评论 / 互动状态。
//!
//! 全部要求登录态（cookie + x-tt-token 随 `api_env` 走），匿名调用会被
//! 服务端静默拒绝——前端应在未登录时禁用入口而非放行重试。

use serde::Deserialize;
use tauri::State;

use hongguo_api::interact::CommentMention;
use hongguo_api::interact::{
    BookshelfEntry, CommentItem, InteractionState, MentionUser, ReplyItem, collect_series,
    collect_series_batch, delete_comment, digg_comment, digg_review, digg_video, fetch_bookshelf,
    fetch_interaction_state, fetch_mention_list, send_comment, send_comment_mention, send_danmaku,
    send_reply, send_review_reply,
};
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;

/// 批量收藏/取消收藏的单条引用。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCollectRef {
    pub series_id: String,
    pub collect: bool,
}

/// 发一条弹幕。`groupId`=分集 vid，`bookId`=series_id，`offsetMs`=视频内
/// 位置（毫秒）。返回服务端 comment_id（本地乐观插入用）。
#[tauri::command]
pub async fn danmaku_send(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    text: String,
    offset_ms: u64,
) -> AppResult<String> {
    let env = state.api_env();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(hongguo_common::AppError::Media("弹幕内容为空".into()));
    }
    let cid = send_danmaku(&group_id, &book_id, &text, offset_ms, &env).await?;
    log::info!("[Interact] danmaku sent group={group_id} offset={offset_ms}ms cid={cid}");
    Ok(cid)
}

/// 发一条播放页评论（与面板同流：单集维度 group=vid / group_type=30）。
/// 返回**服务端回显的完整评论对象**（头像/昵称/uid/时间齐全）——
/// 前端直接插列表顶部。
///
/// `mention_user_id` + `context_comment_ids` 同时在场 = **@提及评论**
/// （2026-10-15/16 官方实包：rich_text tp=15/uri=uid +
/// business_param.aibot_mention_context=最近评论 id 嵌套数组；官方单集
/// 流里的提及评论带 AI 回复，见 flows-official-follow.jsonl:3314）。
/// `mention_user_name` 是被提及者昵称——text 前缀与段文本都是
/// `@昵称\u{FFFC}`，缺了会张冠李戴成「小果」。
#[tauri::command]
pub async fn comment_send(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    text: String,
    mention_user_id: Option<String>,
    mention_user_name: Option<String>,
    context_comment_ids: Option<Vec<String>>,
) -> AppResult<CommentItem> {
    let env = state.api_env();
    let text = text.trim().to_string();
    let mention = match (mention_user_id, mention_user_name, context_comment_ids) {
        // 上下文 id 允许为空（面板无评论时也能 @；官方上下文是软信息，
        // 实包为最近评论 id，缺失不影响提及生效）
        (Some(uid), Some(name), Some(ids)) if !uid.trim().is_empty() => Some(CommentMention {
            user_id: uid.trim().to_string(),
            user_name: name,
            comment_ids: ids,
        }),
        _ => None,
    };
    if text.is_empty() && mention.is_none() {
        return Err(hongguo_common::AppError::Media("评论内容为空".into()));
    }
    match &mention {
        Some(m) => send_comment_mention(&group_id, &book_id, m, &text, &env).await,
        None => send_comment(&group_id, &book_id, &text, &env).await,
    }
}

/// 评论 @ 提及候选列表（含官方 AI bot「小果」，is_ai_bot=true）。
#[tauri::command]
pub async fn mention_list(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Vec<MentionUser>> {
    let env = state.api_env();
    fetch_mention_list(&series_id, &env).await
}

/// 回复一条评论（或一条回复）。回复流与面板同单集维度（group=vid /
/// group_type=30）。`reply_to_reply_id` 回复「回复」时传被回复的那条
/// 回复 id（多级），纯评论回复传空。返回**服务端回显的完整回复对象**——
/// 前端直接插回复区（hgplayer 同款）。
#[tauri::command]
pub async fn comment_reply(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    reply_to_comment_id: String,
    reply_to_reply_id: Option<String>,
    text: String,
) -> AppResult<ReplyItem> {
    let env = state.api_env();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(hongguo_common::AppError::Media("回复内容为空".into()));
    }
    send_reply(
        &group_id,
        &book_id,
        &reply_to_comment_id,
        reply_to_reply_id.as_deref(),
        &text,
        &env,
    )
    .await
}

/// 书架（我的收藏）列表。uid 取登录账号（未登录报错，前端不应调用）。
#[tauri::command]
pub async fn bookshelf_list(state: State<'_, AppState>) -> AppResult<Vec<BookshelfEntry>> {
    let account = state
        .settings()
        .account
        .filter(|a| !a.user_id.is_empty())
        .ok_or_else(|| hongguo_common::AppError::Auth("未登录".into()))?;
    let env = state.api_env();
    fetch_bookshelf(&account.user_id, &env).await
}

/// 点赞 / 取消点赞一集（`vid`=分集 id，`seriesId` 进埋点字段）。
#[tauri::command]
pub async fn video_digg(
    state: State<'_, AppState>,
    vid: String,
    series_id: String,
    digg: bool,
) -> AppResult<()> {
    let env = state.api_env();
    digg_video(&vid, &series_id, digg, &env).await?;
    log::info!("[Interact] digg vid={vid} on={digg}");
    Ok(())
}

/// 点赞 / 取消点赞一条评论（评论区 UI 预留）。
#[tauri::command]
pub async fn comment_digg(
    state: State<'_, AppState>,
    comment_id: String,
    digg: bool,
) -> AppResult<()> {
    let env = state.api_env();
    digg_comment(&comment_id, digg, &env).await
}

/// 点赞 / 取消点赞一条**剧评**（object_type=2 形态，与评论点赞不同路径参数）。
#[tauri::command]
pub async fn review_digg(
    state: State<'_, AppState>,
    review_id: String,
    digg: bool,
) -> AppResult<()> {
    let env = state.api_env();
    digg_review(&review_id, digg, &env).await
}

/// 回复一条**剧评**或剧评的回复（剧评形态 commit_source=13）。
/// `reply_to_reply_id` 回复「回复」时传那条回复的 reply_id，纯剧评回复传空。
#[tauri::command]
pub async fn review_reply_send(
    state: State<'_, AppState>,
    series_id: String,
    reply_to_comment_id: String,
    reply_to_reply_id: Option<String>,
    text: String,
) -> AppResult<ReplyItem> {
    let env = state.api_env();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(hongguo_common::AppError::Media("回复内容为空".into()));
    }
    send_review_reply(
        &series_id,
        &reply_to_comment_id,
        reply_to_reply_id.as_deref(),
        &text,
        &env,
    )
    .await
}

/// 删除自己的评论 / 剧评 / 回复。`service_id`：2 = 剧评，4 = 评论/回复
/// （2026-10-10 抓包锁定）。
#[tauri::command]
pub async fn comment_delete(
    state: State<'_, AppState>,
    comment_id: String,
    service_id: i64,
) -> AppResult<()> {
    let env = state.api_env();
    delete_comment(&comment_id, service_id, &env).await
}

/// 收藏（追剧）或取消收藏一部剧。
#[tauri::command]
pub async fn series_collect(
    state: State<'_, AppState>,
    series_id: String,
    collect: bool,
) -> AppResult<()> {
    let env = state.api_env();
    collect_series(&series_id, collect, &env).await?;
    log::info!("[Interact] collect series={series_id} on={collect}");
    Ok(())
}

/// 批量收藏/取消收藏（一个请求携带多条目，hgplayer 1.1.8 RemoveFavorites 同款）。
#[tauri::command]
pub async fn series_collect_batch(
    state: State<'_, AppState>,
    entries: Vec<SeriesCollectRef>,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("收藏需要先登录".into()));
    }
    let env = state.api_env();
    let pairs: Vec<(String, bool)> = entries
        .into_iter()
        .map(|e| (e.series_id, e.collect))
        .collect();
    let n = pairs.len();
    collect_series_batch(&pairs, &env).await?;
    log::info!("[Interact] batch collect {n} entries");
    Ok(())
}

/// 互动状态列表（最近 100 条：点赞过的 vid + 收藏的 series），前端
/// best-effort 匹配回显。
#[tauri::command]
pub async fn interaction_state(state: State<'_, AppState>) -> AppResult<InteractionState> {
    let env = state.api_env();
    fetch_interaction_state(&env).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn interact_commands_are_async() {
        let src = include_str!("interact_cmd.rs");
        assert!(
            src.contains("pub async fn danmaku_send(")
                && src.contains("pub async fn video_digg(")
                && src.contains("pub async fn series_collect(")
                && src.contains("pub async fn interaction_state("),
            "互动模块应包含全部互动 command 的 async 签名"
        );
    }
}
