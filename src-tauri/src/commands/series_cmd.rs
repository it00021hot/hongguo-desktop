//! 剧集档案：列表与解析。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::AppResult;
use crate::service::series_service;

/// 剧集列表（不含被用户移除的）。
#[tauri::command]
pub fn get_series_list(state: State<'_, AppState>) -> Vec<Series> {
    // 读失败时给空列表而不是报错：列表页不值得为存储抖动弹错误框，
    // 下一次刷新自然会重试。
    state
        .store
        .series_all()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.dismissed)
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
    if let Some(hit) = state.store.series_by_id(&series_id)? {
        return Ok(hit);
    }
    let env = state.api_env();
    let series = series_service::resolver::resolve_series(&series_id, &env).await?;
    series_service::registry::upsert_and_persist(&state, series.clone())?;
    Ok(series)
}

/// 解析链接 / ID 为完整剧集档案并登记。
#[tauri::command]
pub async fn resolve_series(state: State<'_, AppState>, input: String) -> AppResult<Series> {
    let env = state.api_env();
    let series = series_service::resolver::resolve_series(&input, &env).await?;
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

/// 从剧集列表移除一部剧（不删本地文件，也不删已下载的任务记录）。
#[tauri::command]
pub fn remove_series(state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    series_service::dismiss(&state, &series_id)
}

/// 一次性移除列表里的全部剧集，返回移除条数。
#[tauri::command]
pub fn remove_all_series(state: State<'_, AppState>) -> AppResult<usize> {
    series_service::dismiss_all(&state)
}
