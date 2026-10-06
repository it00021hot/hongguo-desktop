//! 剧集档案：列表与解析。

use tauri::{Emitter, State};

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
///
/// 旧格式档案的计数补齐走**后台**（2026-10-06 二次修正）：互动计数是后来
/// 才加进档案的字段，此前入库的档案没有它们。补计数如果挡在返回路径上，
/// 每部剧首看都要等一次网络请求，切剧明显变慢——所以缓存命中**立即返回**，
/// 发现无计数就丢后台任务重解析 + 落库，完成后发
/// `series-archive-updated` 事件，前端据此刷新对应缓存（计数随后浮现）。
#[tauri::command]
pub async fn get_series_episodes(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Series> {
    if let Some(hit) = state.store.series_by_id(&series_id)? {
        let stale = hit
            .episodes
            .first()
            .map(|e| e.comment_count == 0 && e.digg_count == 0)
            .unwrap_or(false);
        if stale {
            let store = state.store.clone();
            let env = state.api_env();
            tauri::async_runtime::spawn(async move {
                match series_service::resolver::resolve_series(&series_id, &env).await {
                    Ok(fresh) => {
                        if let Err(e) = store.upsert_series(&fresh) {
                            log::warn!("[Series] {series_id} 补计数落库失败: {e}");
                            return;
                        }
                        log::info!("[Series] {series_id} 旧档案已在后台补齐计数");
                        let _ = app.emit("series-archive-updated", &series_id);
                    }
                    Err(e) => log::warn!("[Series] {series_id} 后台补计数失败（下次再看再试）: {e}"),
                }
            });
        }
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

/// 拉一部剧的相关作品·系列（同系列各季 + 同 IP，官方 plan 接口）。
///
/// 详情页「相关推荐」tab 顶部的内容；失败由前端静默降级（该模块本就
/// 是增强项，不该为它报错打断推荐 tab）。
#[tauri::command]
pub async fn related_series(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<crate::domain::api::detail::RelatedSeries> {
    let env = state.api_env();
    crate::domain::api::detail::fetch_related_series(&series_id, &env).await
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
