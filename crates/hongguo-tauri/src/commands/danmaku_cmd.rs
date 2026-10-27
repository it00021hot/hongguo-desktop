//! 弹幕 command。

use tauri::State;

use hongguo_api::danmaku::{
    CommentItem, CommentPage, CreatorProfile, CreatorWorksPage, DanmakuWindowPage, ReplyPage,
    SeriesReviewPage, fetch_comment_replies, fetch_comments_page, fetch_creator_profile,
    fetch_creator_works, fetch_danmaku_window_page, fetch_review_replies,
    fetch_series_comments_page, send_series_review,
};
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;

/// 拉一集弹幕的**一窗**（官方滚窗协议；medusa 风控降级下没有
/// 全量拉取路线，壳侧随播放进度逐窗推进——GPUI `advance_danmaku_window`
/// 与 Tauri 播放页滚窗 effect 同语义）。
///
/// `groupId` 是分集 vid，`bookId` 是 series_id，都来自剧集档案的分集表；
/// `startMs` 播放位置窗起点（首页 0，之后传上一窗响应的 `nextStartMs`）；
/// `durationSecs` 该集时长（秒，分集表 duration，body 里换算毫秒）；
/// `cursor` 上一窗响应的 `nextCursor` 原样回传（首页空串）。
#[tauri::command]
pub async fn danmaku_window(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    start_ms: u64,
    duration_secs: i64,
    cursor: String,
) -> AppResult<DanmakuWindowPage> {
    let env = state.api_env();
    let duration_ms = duration_secs.max(0) as u64 * 1000;
    let page =
        fetch_danmaku_window_page(&group_id, &book_id, start_ms, duration_ms, &cursor, &env).await;
    match &page {
        Ok(w) => log::info!(
            "[Danmaku] group={group_id} start={start_ms}ms 窗 {} 条 has_more={} next_start={}",
            w.items.len(),
            w.has_more,
            w.next_start_ms
        ),
        Err(e) => log::warn!("[Danmaku] group={group_id} start={start_ms}ms 拉取失败: {e}"),
    }
    page
}

/// 拉播放页**评论区一页**（单集维度流，官方形态：
/// group=vid / group_type=30 / channel=18 / sort=video_innerflow——
/// 面板整页与互动栏角标同源）。cursor 翻页（响应会话游标原样回传）。
#[tauri::command]
pub async fn comment_list(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    cursor: Option<String>,
) -> AppResult<CommentPage> {
    let env = state.api_env();
    let cursor = cursor.unwrap_or_default();
    match fetch_comments_page(&group_id, &book_id, &cursor, &env).await {
        Ok(page) => {
            log::info!(
                "[Comments] group={group_id} book={book_id} cursor={} 拉到 {} 条 total={}",
                if cursor.is_empty() {
                    "首页"
                } else {
                    "翻页"
                },
                page.items.len(),
                page.total
            );
            Ok(page)
        }
        Err(e) => {
            log::warn!("[Comments] group={group_id} book={book_id} 拉取失败: {e}");
            Err(e)
        }
    }
}

/// 拉**剧级评论**的一页（详情页「剧评」tab：整部剧一条线，不是单集评论）。
///
/// 同端点换形态——`group_type=1`、`comment_source=1`、
/// `server_channel=34`，group_id 是 series_id。响应与单集评论同构。
#[tauri::command]
pub async fn series_comment_list(
    state: State<'_, AppState>,
    series_id: String,
    cursor: Option<String>,
) -> AppResult<SeriesReviewPage> {
    let env = state.api_env();
    let cursor = cursor.unwrap_or_default();
    fetch_series_comments_page(&series_id, &cursor, &env).await
}

/// 发剧评（详情页「剧评」评论框；整剧维度，与 series_comment_list 同组）。
/// score 为十分制评分（5 星 ×2，1–10）。需登录态，匿名会被服务端拒绝
/// （错误原样上抛给 toast）。
#[tauri::command]
pub async fn series_review_send(
    state: State<'_, AppState>,
    series_id: String,
    text: String,
    score: i64,
) -> AppResult<CommentItem> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::InvalidArgs("评论内容不能为空".into()));
    }
    if text.chars().count() > 500 {
        return Err(AppError::InvalidArgs("评论最长 500 字".into()));
    }
    if !(1..=10).contains(&score) {
        return Err(AppError::InvalidArgs(
            "评分需在 1–10 星（5 星制 ×2）".into(),
        ));
    }
    if state.settings().account.is_none() {
        return Err(AppError::Auth("评论需要先登录".into()));
    }
    let env = state.api_env();
    send_series_review(&series_id, &text, score, &env).await
}

/// 拉一条**单集评论**的回复列表一页（回复流与面板同单集维度：
/// group=vid / group_type=30 / channel=18 / source=504）。`cursor` 翻页。
#[tauri::command]
pub async fn comment_replies(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
    comment_id: String,
    cursor: Option<String>,
) -> AppResult<ReplyPage> {
    let env = state.api_env();
    fetch_comment_replies(
        &group_id,
        &book_id,
        &comment_id,
        &cursor.unwrap_or_default(),
        &env,
    )
    .await
}

/// 拉一条**剧评**的回复列表一页（剧评维度 source=501/ch=34）。
#[tauri::command]
pub async fn review_replies(
    state: State<'_, AppState>,
    series_id: String,
    comment_id: String,
    cursor: Option<String>,
) -> AppResult<ReplyPage> {
    let env = state.api_env();
    fetch_review_replies(&series_id, &comment_id, &cursor.unwrap_or_default(), &env).await
}

/// 创作者主页资料头（详情页点创作者 → 主页）。
#[tauri::command]
pub async fn creator_profile(
    state: State<'_, AppState>,
    user_id: String,
) -> AppResult<CreatorProfile> {
    let env = state.api_env();
    fetch_creator_profile(&user_id, &env).await
}

/// 创作者主页作品列表一页（cursor 传上一页返回的原样游标，首页空串）。
#[tauri::command]
pub async fn creator_works(
    state: State<'_, AppState>,
    user_id: String,
    cursor: Option<String>,
) -> AppResult<CreatorWorksPage> {
    let env = state.api_env();
    fetch_creator_works(&user_id, &cursor.unwrap_or_default(), &env).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn danmaku_command_is_async() {
        let src = include_str!("danmaku_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn danmaku_window("))
            .expect("应能找到 danmaku_window 的签名");
        assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
    }
}
