//! 剧集档案：详情解析与相关推荐。

use tauri::{Emitter, State};

use crate::app_state::AppState;
use crate::domain::model::Series;
use crate::error::AppResult;
use crate::service::series_service;

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
        // 缺计数或缺时长（分集时长 2026-10-08 才进档案）都算旧档案
        let stale = hit
            .episodes
            .first()
            .map(|e| {
                (e.comment_count == 0 && e.digg_count == 0)
                    || e.duration == 0
                    // 收藏数 2026-10-09 才进档案：旧档后台补齐后右栏「☆ N」浮现
                    || hit.followed_cnt == 0
            })
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
                    Err(e) => {
                        log::warn!("[Series] {series_id} 后台补计数失败（下次再看再试）: {e}")
                    }
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

/// 解析剧集 ID 为完整剧集档案并登记（收藏/历史/详情的档案回退链路；
/// 分享链接支持已随找剧输入框一起下线）。
#[tauri::command]
pub async fn resolve_series(state: State<'_, AppState>, input: String) -> AppResult<Series> {
    let env = state.api_env();
    let series = series_service::resolver::resolve_series(&input, &env).await?;
    series_service::registry::upsert_and_persist(&state, series.clone())?;
    Ok(series)
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

/// 详情页头部元信息（追剧数/播放量/季徽/题材标签/备案号，官方
/// video_detail 接口；2026-10-07 抓 hgplayer 锁定）。失败静默降级——
/// 头部缺这几行不该打断详情页主功能。
#[tauri::command]
pub async fn series_meta(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<crate::domain::api::detail::SeriesMeta> {
    let env = state.api_env();
    crate::domain::api::detail::fetch_series_meta(&series_id, &env).await
}
