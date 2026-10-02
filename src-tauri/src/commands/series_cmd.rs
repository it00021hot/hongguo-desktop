//! 剧集档案：列表与解析。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::AppResult;
use crate::service::series_service;

/// 剧集列表（不含被用户移除的）。
#[tauri::command]
pub fn get_series_list(state: State<'_, AppState>) -> Vec<Series> {
    state
        .store
        .read()
        .visible_series()
        .into_iter()
        .cloned()
        .collect()
}

/// 取某部剧的完整档案（含分集）。
///
/// 本地命中直接返回；搜索 / 浏览出来的剧从没被解析过，本地查不到时回落
/// [`series_service::resolver::resolve_series`] 解析并登记——否则详情抽屉
/// 只会拿到 `NotFound`，界面上一片空白。
#[tauri::command]
pub async fn get_series_episodes(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Series> {
    if let Some(hit) = state.store.read().series(&series_id).cloned() {
        return Ok(hit);
    }
    let proxy = state.store.read().settings.proxy.clone();
    let series = series_service::resolver::resolve_series(&series_id, &proxy).await?;
    series_service::registry::upsert_and_persist(&state, series.clone())?;
    Ok(series)
}

/// 解析链接 / ID 为完整剧集档案并登记。
#[tauri::command]
pub async fn resolve_series(state: State<'_, AppState>, input: String) -> AppResult<Series> {
    let proxy = state.store.read().settings.proxy.clone();
    let series = series_service::resolver::resolve_series(&input, &proxy).await?;
    series_service::registry::upsert_and_persist(&state, series.clone())?;
    Ok(series)
}

/// 详情页附加信息（简介 + 推荐）。按需取，不落盘。
#[tauri::command]
pub async fn get_series_extras(series_id: String) -> crate::domain::model::SeriesExtras {
    let url = format!("https://hongguoduanju.com/detail?series_id={series_id}");
    match crate::domain::site::fetch::fetch_site_html(&url).await {
        Ok(html) => crate::domain::model::SeriesExtras {
            intro: crate::domain::site::series_page::intro_from_html(&html),
            recommendations: crate::domain::site::series_page::recommendations_from_html(&html),
        },
        Err(e) => {
            log::warn!("[Series] 详情页附加信息取不到: {e}");
            Default::default()
        }
    }
}
