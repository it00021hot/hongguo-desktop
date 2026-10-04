//! 弹幕 command。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::danmaku::{fetch_danmaku_all, Danmaku};
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
