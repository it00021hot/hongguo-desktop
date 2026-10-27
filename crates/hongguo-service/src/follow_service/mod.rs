//! 追更服务：开关 / 列表 / 更新检查（对齐 hongguo-player v1.2.1，抓包口径见
//! captures/capture-requests-20261015.md）。
//!
//! 语义：
//! - **登录墙**：追更必须登录态（服务端跟随列表），未登录报 `Auth` 透传原文；
//! - **是否已追没有独立接口**——拉追更列表第一页（count=20）本地判成员
//!   （player 同款；追超过 20 部时老剧会误判未追，接受此口径）；
//! - **更新检查**：列表条目的服务端总集数 vs 本地基线，出新集落 `follow_notice`
//!   并发事件；基线不存在（首次见到）只记基线不提示。取消追更删基线，重追
//!   从当前集数重新起算，不补发历史更新；
//! - **通知检查定时器**：启动后立即查一次，之后每 20 分钟一次（player 同款
//!   周期）；未登录时空转，登录后自动开始出通知。

pub mod events;

use hongguo_api::interact::{FollowUpdateItem, FollowUpdatePage};
use hongguo_common::model::{FollowNotice, NOTICE_KIND_FOLLOW};
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
    login_wall(state)?;
    let env = state.api_env();
    hongguo_api::interact::follow_update(series_id, follow, &env).await?;
    if follow {
        // 立即记基线为当前集数：从现在起出新集才提示（服务端条目下次检查
        // 会带最新 episode_cnt，基线抢先写当前值兜住「追完马上检查」的窗口）
        let _ = refresh_baseline(state, series_id).await;
    } else {
        state.store.follow_baseline_delete(series_id)?;
    }
    Ok(())
}

/// 拉追更列表一页（cursor 翻页；首页空串）。
pub async fn list(state: &AppState, cursor: &str) -> AppResult<FollowUpdatePage> {
    login_wall(state)?;
    let env = state.api_env();
    hongguo_api::interact::fetch_follow_updates(cursor, &env).await
}

/// 是否已追（列表第一页本地判成员，player IsFollowingUpdate 同款口径）。
pub async fn is_following(state: &AppState, series_id: &str) -> AppResult<bool> {
    login_wall(state)?;
    let env = state.api_env();
    let page = hongguo_api::interact::fetch_follow_updates("", &env).await?;
    Ok(page.items.iter().any(|i| i.series_id == series_id))
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

/// 更新检查一轮：出新集落通知 + 发事件，返回本轮新增通知数。
/// 未登录直接返回 0（定时器场景不报错刷屏；手动「检查更新」由 command 层
/// 决定是否要先过登录墙提示）。
pub async fn check_updates(state: &AppState) -> AppResult<usize> {
    if state.settings().account.is_none() {
        return Ok(0);
    }
    let env = state.api_env();
    let page = hongguo_api::interact::fetch_follow_updates("", &env).await?;
    persist_new_notices(state, &page.items).await
}

/// 对一组追更条目做基线比对，出新集的落通知。返回新增通知数。
async fn persist_new_notices(state: &AppState, items: &[FollowUpdateItem]) -> AppResult<usize> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut fresh: Vec<FollowNotice> = Vec::new();
    for item in items {
        let Some(baseline) = state.store.follow_baseline_get(&item.series_id)? else {
            // 首次见到：只记基线，不提示（追更前已经出的集不算「更新」）
            state
                .store
                .follow_baseline_set(&item.series_id, item.episode_cnt)?;
            continue;
        };
        if item.episode_cnt <= baseline {
            continue;
        }
        let added = item.episode_cnt - baseline;
        let title = if item.title.is_empty() {
            "追更更新"
        } else {
            item.title.as_str()
        };
        let detail = format!(
            "追的剧更新了，新增 {added} 集（共 {} 集）",
            item.episode_cnt
        );
        let id = state.store.follow_notice_insert(
            NOTICE_KIND_FOLLOW,
            &item.series_id,
            title,
            &detail,
            now,
        )?;
        state
            .store
            .follow_baseline_set(&item.series_id, item.episode_cnt)?;
        fresh.push(FollowNotice {
            id,
            kind: NOTICE_KIND_FOLLOW.into(),
            series_id: item.series_id.clone(),
            title: title.to_string(),
            detail,
            at: now,
            read: false,
        });
    }
    if !fresh.is_empty() {
        let count = fresh.len();
        state
            .events()
            .publish(crate::events::ServiceEvent::FollowNotices(
                events::FollowNoticesEvent { notices: fresh },
            ));
        return Ok(count);
    }
    Ok(0)
}

/// 追更动作后立即把基线对到服务端当前值（best-effort：失败不打断追更本身，
/// 下次检查会用「无基线→记基线」路径兜住）。
async fn refresh_baseline(state: &AppState, series_id: &str) -> AppResult<()> {
    let env = state.api_env();
    let page = hongguo_api::interact::fetch_follow_updates("", &env).await?;
    if let Some(item) = page.items.iter().find(|i| i.series_id == series_id) {
        state
            .store
            .follow_baseline_set(series_id, item.episode_cnt)?;
    }
    Ok(())
}

/// 启动通知检查定时器：先立即查一轮，之后每 [`CHECK_INTERVAL`] 一轮。
/// 壳装配期调用一次（两壳共用）；任务常驻到进程结束。
pub fn start_notice_loop(state: crate::app_state::AppState) {
    crate::runtime::spawn(async move {
        loop {
            if let Err(e) = check_updates(&state).await {
                log::info!("[Follow] 更新检查失败（下轮重试）: {e}");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}
