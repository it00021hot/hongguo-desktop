//! 官方 rx4.q 选档的有状态部分：手选作用域、模型缓存命中和服务端规则存活期。
use std::collections::HashMap;
use std::time::Instant;

use chrono::{Local, Timelike};
use hongguo_api::resolution_policy::{HeldStrategy, Purpose, Rules, Selection, Strategy};
use hongguo_common::{AppError, AppResult};
use parking_lot::Mutex;
use tokio::sync::OnceCell;

use crate::app_state::AppState;

#[derive(Default)]
pub(crate) struct ResolutionState {
    memory: Mutex<HashMap<String, u32>>,
    controller: Mutex<Controller>,
    pub(super) poller: OnceCell<()>,
}

#[derive(Default)]
struct Controller {
    generation: u64,
    strategy: Strategy,
    held: HeldStrategy,
    cached: Option<(String, Instant, u32)>,
}

fn scope(rules: &Rules, series: &str) -> String {
    if rules.series.single_series {
        series.to_owned()
    } else {
        String::new()
    }
}

/// 只在用户明确选择时更新；自动请求不覆盖其他集/剧留下的记忆。
pub(super) async fn remember(
    state: &AppState,
    series: &str,
    value: Option<u32>,
    reset: bool,
) -> AppResult<()> {
    if value.is_none() && !reset {
        return Ok(());
    }
    let policy = super::prefetch::policy(state).await;
    remember_with_rules(state, &policy.rules.resolution, series, value)
}

fn remember_with_rules(
    state: &AppState,
    rules: &Rules,
    series: &str,
    value: Option<u32>,
) -> AppResult<()> {
    let scope = scope(rules, series);
    if rules.series.single_progress {
        let mut memory = state.playback_resolution.memory.lock();
        if let Some(value) = value {
            memory.insert(scope, value);
        } else {
            memory.remove(&scope);
        }
        Ok(())
    } else {
        state.store.save_playback_definition(&scope, value)
    }
}

fn preferred(state: &AppState, rules: &Rules, series: &str) -> AppResult<Option<u32>> {
    let scope = scope(rules, series);
    if rules.series.single_progress {
        Ok(state.playback_resolution.memory.lock().get(&scope).copied())
    } else if rules.series.show {
        state.store.playback_definition(&scope)
    } else {
        Ok(None)
    }
}

pub(super) fn query_gap(state: &AppState) -> u64 {
    let gap = state
        .playback_resolution
        .controller
        .lock()
        .strategy
        .query_gap;
    if gap == 0 {
        hongguo_api::play_strategy::DEFAULT_QUERY_GAP_SECS
    } else {
        gap
    }
}

pub(super) fn accept_strategy(state: &AppState, generation: u64, strategy: Strategy) {
    if state.device_generation() != generation {
        return;
    }
    let mut controller = state.playback_resolution.controller.lock();
    if controller.generation != generation {
        *controller = Controller {
            generation,
            ..Controller::default()
        };
    }
    controller.strategy = strategy;
}

/// 取流表到达后再择档；不会再请求一次 URL，也不会把手选流当成“自动最高档”。
pub(super) async fn select(
    state: &AppState,
    vid: &str,
    progress_key: &str,
    explicit: Option<u32>,
    purpose: Purpose,
) -> AppResult<hongguo_api::play_url::PlayInfo> {
    let policy = super::prefetch::policy(state).await;
    let rules = &policy.rules.resolution;
    super::ensure_play_strategy_poller(state).await;
    let cap = crate::device::stream_cap();
    let (stamp, mut candidates) = hongguo_api::play_url::playback_candidates(vid, cap)
        .ok_or_else(|| AppError::Media("播放选档缺少有效流表".into()))?;
    // getSupportResolutions 按 Resolution 枚举次序返回，与 video_list 的码率顺序无关。
    candidates.sort_by_key(|value| value.definition);
    let (series, episode) = progress_key.split_once(':').unwrap_or((progress_key, ""));
    let preferred = explicit.or(preferred(state, rules, series)?);
    let generation = state.device_generation();
    let remembered_cache = {
        let mut controller = state.playback_resolution.controller.lock();
        if controller.generation != generation {
            *controller = Controller {
                generation,
                ..Controller::default()
            };
        }
        controller
            .cached
            .as_ref()
            .filter(|(key, model, _)| key == vid && *model == stamp)
            .map(|(_, _, definition)| *definition)
    };
    let cached = if !rules.series.opt_cache || preferred.is_some() {
        None
    } else if remembered_cache.is_some() {
        remembered_cache
    } else {
        let device = state.device();
        let key = vid.to_owned();
        let streams = candidates.clone();
        crate::runtime::spawn_blocking(move || {
            super::online::disk::cached_definition(&device, &key, &streams)
        })
        .await
        .map_err(|error| AppError::Media(format!("读取选档缓存失败: {error}")))?
    };
    let time = Local::now();
    let predictor_bps = hongguo_api::resolution_policy::network_speed();
    let environment = hongguo_api::resolution_policy::Environment {
        wifi: state.device().get("ac") == "wifi",
        peak: policy.is_peak(time.hour() * 60 + time.minute()),
        episode: episode.parse().unwrap_or(-1),
        predictor_bps,
        target_mbps: predictor_bps / 1_000_000.0,
        // Android 的 low/middle-low 设备名单不能套用到桌面硬件。
        ..Default::default()
    };
    let selected = {
        let mut controller = state.playback_resolution.controller.lock();
        if let Some(definition) = cached {
            controller.cached = Some((vid.to_owned(), stamp, definition));
        }
        let Controller { strategy, held, .. } = &mut *controller;
        rules.select(
            &Selection {
                candidates: &candidates,
                preferred,
                cached,
                purpose,
                strategy,
                environment,
                now_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            },
            held,
        )
    }
    .ok_or_else(|| AppError::Media("本集没有可播放的清晰度档位".into()))?;
    log::info!("[Resolution] {vid} {purpose:?} 手选 {preferred:?} 缓存 {cached:?} -> {selected}P");
    hongguo_api::play_url::cached_play(vid, Some(selected), cap)
        .filter(|play| play.definition == selected)
        .ok_or_else(|| AppError::Media(format!("选中的 {selected}P 流已失效")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_memory_obeys_process_series_and_persistent_flags() {
        let state = AppState::default();
        let mut rules = Rules::default();
        remember_with_rules(&state, &rules, "A", Some(1080)).unwrap();
        assert_eq!(preferred(&state, &rules, "B").unwrap(), Some(1080));
        rules.series.single_series = true;
        assert_eq!(preferred(&state, &rules, "A").unwrap(), None);
        remember_with_rules(&state, &rules, "A", Some(540)).unwrap();
        assert_eq!(preferred(&state, &rules, "A").unwrap(), Some(540));
        assert_eq!(preferred(&state, &rules, "B").unwrap(), None);
        rules.series.single_progress = false;
        assert_eq!(preferred(&state, &rules, "A").unwrap(), None);
        remember_with_rules(&state, &rules, "A", Some(480)).unwrap();
        let reopened = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(
            state.store.clone(),
        ));
        assert_eq!(preferred(&reopened, &rules, "A").unwrap(), Some(480));
        rules.series.show = false;
        assert_eq!(preferred(&reopened, &rules, "A").unwrap(), None);
        rules.series.show = true;
        remember_with_rules(&reopened, &rules, "A", None).unwrap();
        assert_eq!(preferred(&state, &rules, "A").unwrap(), None);
        // 清除持久化作用域不应顺带清除进程记忆。
        rules.series.single_progress = true;
        assert_eq!(preferred(&state, &rules, "A").unwrap(), Some(540));
        assert_eq!(preferred(&reopened, &rules, "A").unwrap(), None);
    }
}
