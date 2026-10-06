//! 互动 command：点赞 / 收藏 / 发弹幕 / 发评论 / 互动状态。
//!
//! 全部要求登录态（cookie + x-tt-token 随 `api_env` 走），匿名调用会被
//! 服务端静默拒绝——前端应在未登录时禁用入口而非放行重试。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::interact::{
    collect_series, digg_comment, digg_video, fetch_bookshelf, fetch_interaction_state, send_comment,
    send_danmaku, send_reply, BookshelfEntry, InteractionState,
};
use crate::error::AppResult;

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
        return Err(crate::error::AppError::Media("弹幕内容为空".into()));
    }
    let cid = send_danmaku(&group_id, &book_id, &text, offset_ms, &env).await?;
    log::info!("[Interact] danmaku sent group={group_id} offset={offset_ms}ms cid={cid}");
    Ok(cid)
}

/// 发一条评论（offset 恒 0）。返回 comment_id。
#[tauri::command]
pub async fn comment_send(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    text: String,
) -> AppResult<String> {
    let env = state.api_env();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(crate::error::AppError::Media("评论内容为空".into()));
    }
    send_comment(&group_id, &book_id, &text, &env).await
}

/// 回复一条评论（或一条回复）。`reply_to_reply_id` 回复「回复」时传
/// 被回复的那条回复 id（多级），纯评论回复传空。返回 reply_id。
#[tauri::command]
pub async fn comment_reply(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    reply_to_comment_id: String,
    reply_to_reply_id: Option<String>,
    text: String,
) -> AppResult<String> {
    let env = state.api_env();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(crate::error::AppError::Media("回复内容为空".into()));
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
        .ok_or_else(|| crate::error::AppError::Auth("未登录".into()))?;
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
        for (name, src) in [
            ("danmaku_send", include_str!("interact_cmd.rs")),
        ] {
            let _ = name;
            assert!(
                src.contains("pub async fn danmaku_send(")
                    && src.contains("pub async fn video_digg(")
                    && src.contains("pub async fn series_collect(")
                    && src.contains("pub async fn interaction_state("),
                "{name} 模块应包含全部互动 command 的 async 签名"
            );
        }
    }
}
