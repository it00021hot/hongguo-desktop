//! 播放服务：本地播放、在线播放、断点续播。

use crate::app_state::AppState;

mod feed;
pub mod local;
pub mod online;
pub mod position;
mod prefetch;
pub(crate) mod resolution;
pub mod resume;

pub use feed::prefetch_feed;
pub use hongguo_api::prefetch_policy::DEFAULT_PREFETCH_COUNT;
pub(crate) use prefetch::PrefetchCache;
pub use prefetch::prefetch_budget;

/// 当前在线集起播后，向后铺同一部剧的后续集。两个壳从同一入口调度：
/// 在线预取按动态字节预算铺媒体。
pub fn schedule_following_episodes(state: &AppState, series_id: &str, current_vid_index: u32) {
    let online_state = state.clone();
    let sid = series_id.to_string();
    crate::runtime::spawn(async move {
        let budget = prefetch_budget(&online_state).await;
        let count = budget.count;
        if count == 0 {
            return;
        }
        local::prefetch_episode_models(
            &online_state,
            &sid,
            current_vid_index.saturating_add(1)..=current_vid_index.saturating_add(count),
            hongguo_api::params::PlayerModelScene::Player,
        )
        .await;
        for offset in 1..=count {
            online::schedule_next_episode_prefetch(
                online_state.clone(),
                &sid,
                current_vid_index.saturating_add(offset),
                budget.bytes,
            );
            if offset < count {
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                // 预取错峰：1~5s 抖动，避免与起播抢带宽
                const PREFETCH_DELAY_MIN_MS: u64 = 1000;
                const PREFETCH_DELAY_SPAN_MS: u64 = 4000;
                let delay = PREFETCH_DELAY_MIN_MS + (ns as u64 % PREFETCH_DELAY_SPAN_MS);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }
    });
}

/// 首次选档等首包策略；之后按 query_gap 更新同一控制器，失败保留上次有效值。
pub async fn ensure_play_strategy_poller(state: &AppState) {
    state
        .playback_resolution
        .poller
        .get_or_init(|| async {
            let gap = refresh_play_strategy(state).await;
            let state = state.clone();
            crate::runtime::spawn(async move {
                let mut gap = gap;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(gap)).await;
                    gap = refresh_play_strategy(&state).await;
                }
            });
        })
        .await;
}

async fn refresh_play_strategy(state: &AppState) -> u64 {
    let generation = state.device_generation();
    match hongguo_api::play_strategy::fetch_play_strategy(&state.api_env()).await {
        Ok(strategy) => {
            let gap = strategy.query_gap.max(1);
            resolution::accept_strategy(state, generation, strategy);
            log::debug!("[PlayStrategy] 策略已更新，轮询间隔 {gap}s");
            gap
        }
        Err(error) => {
            log::warn!("[PlayStrategy] 拉取失败，保留有效策略: {error}");
            resolution::query_gap(state)
        }
    }
}
