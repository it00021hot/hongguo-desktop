//! 发现类 command：首页推荐流、找剧（筛选浏览）。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::discover::{
    fetch_browse, fetch_browse_panel, BrowseFilters, FeedPage, SelectorRow,
};
use crate::domain::api::rank::fetch_recommend_feed;
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

/// 拉一页首页推荐流（书城换一换，hgplayer RecommendTab 同源）。
///
/// `tab`：'16'=推荐、'36'=漫剧、'39'=真人剧（rank.rs fetch_recommend_feed
/// 文档有 tab 表与两段式流程）。`sessionId` 首页传空串（走 bookmall/tab
/// cr=4，响应下发会话与首批）；翻页传上一页的会话 + `offset`（上一页的
/// nextOffset）+ `filterIds`（已下发过的 series_id，服务端排除已见——
/// hgplayer 同款）。
#[tauri::command]
pub async fn recommend_feed(
    state: State<'_, AppState>,
    tab: String,
    session_id: Option<String>,
    offset: Option<i64>,
    filter_ids: Option<Vec<String>>,
) -> AppResult<FeedPage> {
    let env = state.api_env();
    let result = fetch_recommend_feed(
        &tab,
        session_id.as_deref().unwrap_or(""),
        offset.unwrap_or(0),
        filter_ids.as_deref().unwrap_or_default(),
        &env,
    )
    .await;
    match &result {
        Ok(page) => log::info!(
            "[Recommend] tab={tab} -> {} 条, 首条: {}",
            page.items.len(),
            page.items.first().map(|i| i.title.as_str()).unwrap_or("-")
        ),
        Err(e) => log::warn!("[Recommend] tab={tab} 失败: {e}"),
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
        for name in ["recommend_feed", "browse_page"] {
            let sig = src
                .lines()
                .find(|l| l.contains(&format!("fn {name}(")))
                .unwrap_or_else(|| panic!("应能找到 {name} 的签名"));
            assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
        }
    }
}
