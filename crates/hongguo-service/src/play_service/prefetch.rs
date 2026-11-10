use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};
use hongguo_api::prefetch_policy::{PrefetchBudget, PrefetchPolicy, fetch_prefetch_rules};
use parking_lot::Mutex;
use tokio::sync::OnceCell;

use crate::app_state::AppState;

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

pub(crate) struct PrefetchCache(Mutex<CachedPolicy>);

struct CachedPolicy {
    generation: u64,
    refresh_at: Instant,
    cell: Arc<OnceCell<PrefetchPolicy>>,
    previous: Option<PrefetchPolicy>,
}

impl CachedPolicy {
    fn new(generation: u64, now: Instant) -> Self {
        Self {
            generation,
            refresh_at: now + REFRESH_INTERVAL,
            cell: Arc::new(OnceCell::new()),
            previous: None,
        }
    }
}

impl Default for PrefetchCache {
    fn default() -> Self {
        Self(Mutex::new(CachedPolicy::new(0, Instant::now())))
    }
}

pub async fn prefetch_budget(state: &AppState) -> PrefetchBudget {
    let generation = state.device_generation();
    let now = Instant::now();
    let (cell, previous) = {
        let mut slot = state.prefetch_policy.0.lock();
        if slot.generation != generation {
            *slot = CachedPolicy::new(generation, now);
        } else if now >= slot.refresh_at
            && let Some(last) = slot.cell.get().cloned()
        {
            *slot = CachedPolicy {
                previous: Some(last),
                ..CachedPolicy::new(generation, now)
            };
        }
        (Arc::clone(&slot.cell), slot.previous.clone())
    };
    let env = state.api_env();
    let policy = cell
        .get_or_init(|| async {
            let rules =
                fetch_prefetch_rules(&env, previous.as_ref().map(|value| &value.rules)).await;
            let mut slot = state.prefetch_policy.0.lock();
            if Arc::ptr_eq(&slot.cell, &cell) {
                slot.refresh_at = Instant::now() + REFRESH_INTERVAL;
            }
            drop(slot);
            if let Some(mut previous) = previous.filter(|value| value.rules.same_policy(&rules)) {
                previous.rules = rules;
                previous
            } else {
                PrefetchPolicy::from_rules(rules)
            }
        })
        .await;
    let local = Local::now();
    let minute = local.hour() * 60 + local.minute();
    let wifi = env.device.get("ac") == "wifi";
    let budget = policy.select(minute, wifi);
    log::debug!(
        "[Prefetch] {:?} 窗口 {}，媒体上限 {} 字节",
        budget.phase,
        budget.count,
        budget.bytes
    );
    budget
}
