//! 剧集档案：详情解析与相关推荐。
//!
//! 分集表与快照两条读取策略（2026-10-16 定型）：
//! - [`get_series_episodes`]（选集/详情/播放器）**在线总是现拉**——连载剧
//!   随时在更新，本地快照的集数一过夜就是错的，靠任何「事件时刷新」都有
//!   覆盖洞（追更检查只看第一页 20 部、没追更的剧无人触发）。现拉失败
//!   （离线/接口抖动）回落快照：下载了本片的剧离线也要能看分集列表。
//! - [`get_series_snapshot`]（点赞/收藏列表行）**快照优先**——列表行只要
//!   封面/标题，快照命中秒回零网络；从没解析过的剧回落解析一次并归档。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_common::model::Series;
use hongguo_service::app_state::AppState;
use hongguo_service::series_service;

/// 取某部剧的完整档案（含分集）。在线现拉并归档，失败回落本地快照。
#[tauri::command]
pub async fn get_series_episodes(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Series> {
    let env = state.api_env();
    match series_service::resolver::resolve_series(&series_id, &env).await {
        Ok(fresh) => {
            series_service::registry::upsert_and_persist(&state, fresh.clone())?;
            Ok(fresh)
        }
        Err(e) => {
            // 现拉失败回落快照：离线时下载过的剧仍能看到分集（在线的流
            // 反正也取不到，点开自会走播放错误路径）。无快照才把错误上抛。
            match state.store.series_by_id(&series_id) {
                Ok(Some(hit)) => {
                    log::warn!("[Series] {series_id} 现拉失败，回落本地快照: {e}");
                    Ok(hit)
                }
                _ => Err(e),
            }
        }
    }
}

/// 取某部剧的本地快照（列表页封面/标题用）。命中秒回；从没解析过的剧
/// 回落 [`series_service::resolver::resolve_series`] 解析并登记——否则
/// 点赞/收藏行只会拿到 `NotFound`，封面标题一片空白。
#[tauri::command]
pub async fn get_series_snapshot(
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
) -> AppResult<hongguo_api::detail::RelatedSeries> {
    let env = state.api_env();
    hongguo_api::detail::fetch_related_series(&series_id, &env).await
}

/// 详情页头部元信息（追剧数/播放量/季徽/题材标签/备案号，官方
/// video_detail 接口；2026-10-07 抓包锁定）。失败静默降级——
/// 头部缺这几行不该打断详情页主功能。
#[tauri::command]
pub async fn series_meta(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<hongguo_api::detail::SeriesMeta> {
    let env = state.api_env();
    hongguo_api::detail::fetch_series_meta(&series_id, &env).await
}
