//! 弹幕 command。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::danmaku::{
    fetch_comments_page, fetch_danmaku_all, CommentPage, Danmaku,
};
use crate::error::AppResult;

/// 拉一集的全部弹幕（后端按 30 秒窗口循环到 has_more=false）。
///
/// `groupId` 是分集 vid，`bookId` 是 series_id，都来自剧集档案的分集表。
#[tauri::command]
pub async fn danmaku_list(
    state: State<'_, AppState>,
    group_id: String,
    book_id: String,
) -> AppResult<Vec<Danmaku>> {
    let env = state.api_env();
    match fetch_danmaku_all(&group_id, &book_id, &env).await {
        Ok(list) => {
            let preview: Vec<String> = list
                .iter()
                .take(5)
                .map(|d| format!("{}ms·{:?}", d.offset_ms, d.text))
                .collect();
            log::info!(
                "[Danmaku] group={group_id} book={book_id} 拉到 {} 条{}",
                list.len(),
                if preview.is_empty() { String::new() } else { format!("（前几条: {}）", preview.join(" | ")) }
            );
            Ok(list)
        }
        Err(e) => {
            log::warn!("[Danmaku] group={group_id} book={book_id} 拉取失败: {e}");
            Err(e)
        }
    }
}


/// 拉一集的**评论区一页**（ct=4/src=4，一窗 20 条；cursor 翻页）。
/// 返回列表 + 评论总数（`total` 是互动栏评论计数的数据源）+ 翻页游标——
/// 第一页秒回，不再像旧版那样拉完整集才返回（热门集转圈 30s）。
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
                if cursor.is_empty() { "首页" } else { "翻页" },
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

#[cfg(test)]
mod tests {
    #[test]
    fn danmaku_command_is_async() {
        let src = include_str!("danmaku_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn danmaku_list("))
            .expect("应能找到 danmaku_list 的签名");
        assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
    }
}
