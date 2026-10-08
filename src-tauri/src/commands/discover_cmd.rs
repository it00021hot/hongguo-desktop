//! 发现类 command：推荐信息流、找剧（筛选浏览）。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::discover::{
    fetch_browse, fetch_browse_panel, fetch_feed, BrowseFilters, FeedPage, SelectorRow,
};
use crate::error::AppResult;

/// 拉找剧筛选面板（八行维度选项，选项表随服务端运营变化，不落死）。
#[tauri::command]
pub async fn browse_panel(state: State<'_, AppState>) -> AppResult<Vec<SelectorRow>> {
    let env = state.api_env();
    fetch_browse_panel(&env).await
}

/// 拉一页找剧结果（多维筛选，服务端过滤）。
///
/// `filters` 八个维度各至多一个选中值（空串 = 全部）；`sessionId`
/// 首页传空串，翻页传上一页响应里的值。
#[tauri::command]
pub async fn browse_page(
    state: State<'_, AppState>,
    filters: BrowseFilters,
    offset: Option<i64>,
    session_id: Option<String>,
) -> AppResult<FeedPage> {
    let env = state.api_env();
    fetch_browse(
        &filters,
        offset.unwrap_or(0),
        session_id.as_deref().unwrap_or(""),
        &env,
    )
    .await
}

/// 拉一页推荐信息流（官方 body 协议，服务端按体裁过滤）。
///
/// `offset` 不给（或给 0）取首页；翻页传上一页的 `nextOffset`。
/// `genre`：`comic_series`=漫剧、`short_play`=真人剧、`ai_series`=AI剧，
/// 不传=全部体裁。
#[tauri::command]
pub async fn discover_feed(
    state: State<'_, AppState>,
    offset: Option<i64>,
    genre: Option<String>,
) -> AppResult<FeedPage> {
    let env = state.api_env();
    let result = fetch_feed(offset.unwrap_or(0), genre.as_deref(), &env).await;
    match &result {
        Ok(page) => log::info!(
            "[Feed] genre={:?} offset={} -> {} 条, 首条: {}",
            genre,
            offset.unwrap_or(0),
            page.items.len(),
            page.items.first().map(|i| i.title.as_str()).unwrap_or("-")
        ),
        Err(e) => log::warn!(
            "[Feed] genre={:?} offset={} 失败: {e}",
            genre,
            offset.unwrap_or(0)
        ),
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn feed_command_is_async() {
        // 直连真实接口的命令必须 async：同步 command 会占住主线程，
        // 接口慢时整个窗口卡死（与 merge_series 同一条纪律）
        let src = include_str!("discover_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn discover_feed("))
            .expect("应能找到 discover_feed 的签名");
        assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
    }
}
