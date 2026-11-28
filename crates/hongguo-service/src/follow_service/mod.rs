//! 追更服务：开关 / 列表 / 更新检查（对齐 hongguo-player，抓包口径见 captures/）。
//! 更新检查比对服务端总集数与本地基线，出新集落通知并发事件；
//! 基线不存在只记基线，取消追更删基线。

mod check;
pub mod events;

use hongguo_api::interact::FollowUpdatePage;
use hongguo_common::model::FollowNotice;
use hongguo_common::{AppError, AppResult};

use crate::app_state::AppState;

/// 更新检查周期（player 1.1.8 空态文案明示「程序每 20 分钟检查一次」）。
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20 * 60);

fn login_wall(state: &AppState) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("登录后才能追更".into()));
    }
    Ok(())
}

/// 追更 / 取消追更一部剧。取消时同时删本地基线（重追从当前集数重新起算）。
pub async fn toggle(state: &AppState, series_id: &str, follow: bool) -> AppResult<()> {
    let (scope, env) =
        check::Scope::capture(state).ok_or_else(|| AppError::Auth("登录后才能追更".into()))?;
    hongguo_api::interact::follow_update(series_id, follow, &env).await?;
    if follow {
        let _ = refresh_baseline(state, &scope, &env, series_id).await;
    } else {
        let id = series_id.to_owned();
        scope
            .store(state, move |store| store.follow_baseline_delete(&id))
            .await?;
    }
    Ok(())
}

/// 拉追更列表一页（cursor 翻页；首页空串）。
pub async fn list(state: &AppState, cursor: &str) -> AppResult<FollowUpdatePage> {
    login_wall(state)?;
    let env = state.api_env();
    hongguo_api::interact::fetch_follow_updates(cursor, &env).await
}

/// 拉本地通知（新→旧）。
pub fn notices(state: &AppState) -> AppResult<Vec<FollowNotice>> {
    state.store.follow_notices()
}

/// 标记通知已读，返回改动行数。
pub fn mark_notices_read(state: &AppState, ids: &[i64]) -> AppResult<u64> {
    state.store.follow_notices_mark_read(ids)
}

pub fn clear_notices(state: &AppState) -> AppResult<()> {
    state.store.follow_notices_clear()
}

/// 手动检查完整追更列表；没有登录态时返回 0。
pub async fn check_updates(state: &AppState) -> AppResult<usize> {
    check::manual(state).await
}

/// 追更动作后立即把基线对到服务端当前值（best-effort：失败不打断追更本身，
/// 下次检查会用「无基线→记基线」路径兜住）。
async fn refresh_baseline(
    state: &AppState,
    scope: &check::Scope,
    env: &hongguo_api::client::ApiEnv,
    series_id: &str,
) -> AppResult<()> {
    let page = hongguo_api::interact::fetch_follow_updates("", env).await?;
    if let Some(item) = page
        .items
        .into_iter()
        .find(|i| i.series_id == series_id && i.episode_cnt > 0)
    {
        scope
            .store(state, move |store| {
                store.follow_baseline_set(&item.series_id, item.episode_cnt)
            })
            .await?;
    }
    Ok(())
}

/// 启动通知检查任务：先立即查一轮，之后每 `CHECK_INTERVAL` 一轮。
/// The owner must abort this task when its application session ends.
pub fn start_notice_task(state: AppState) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn(async move {
        let mut scan = check::BackgroundScan::default();
        loop {
            if let Err(e) = scan.tick(&state).await
                && !matches!(e, AppError::Cancelled)
            {
                log::info!("[Follow] 更新检查失败（下轮重试）: {e}");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    })
}
