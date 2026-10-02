//! 剧集档案：列表、解析、移除。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::{AppError, AppResult};
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
#[tauri::command]
pub fn get_series_episodes(state: State<'_, AppState>, series_id: String) -> AppResult<Series> {
    state
        .store
        .read()
        .series(&series_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("剧集 {series_id}")))
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

/// 从列表移除（不删本地文件）。
#[tauri::command]
pub fn remove_series(state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    series_service::dismiss(&mut state.store.write(), &series_id)?;
    persist(&state);
    Ok(())
}

/// 恢复被移除的剧集。
#[tauri::command]
pub fn restore_dismissed_series(state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    series_service::restore(&mut state.store.write(), &series_id)?;
    persist(&state);
    Ok(())
}

/// 已被移除的剧集数。
#[tauri::command]
pub fn dismissed_count(state: State<'_, AppState>) -> usize {
    series_service::dismissed_list(&state.store.read()).len()
}

/// 清理没有任何分集记录的剧集档案。
#[tauri::command]
pub fn purge_empty_series(state: State<'_, AppState>) -> AppResult<usize> {
    let mut data = state.store.write();
    let before = data.series.len();
    data.series.retain(|s| !s.episodes.is_empty());
    let removed = before - data.series.len();
    if removed > 0 {
        data.save(&crate::store::paths::data_file())
            .map_err(|e| AppError::StoreCorrupt(e.to_string()))?;
    }
    Ok(removed)
}

fn persist(state: &State<'_, AppState>) {
    if let Err(e) = state.store.read().save(&crate::store::paths::data_file()) {
        log::error!("[Series] 落盘失败: {e}");
    }
}
