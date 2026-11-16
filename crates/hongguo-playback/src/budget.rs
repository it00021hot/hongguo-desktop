use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use hongguo_common::{AppError, AppResult};
use parking_lot::{Condvar, Mutex};

pub(crate) fn available_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

struct Budget {
    limit: usize,
    used: Mutex<usize>,
    changed: Condvar,
}

fn budget() -> &'static Budget {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    BUDGET.get_or_init(|| Budget {
        limit: available_threads(),
        used: Mutex::new(0),
        changed: Condvar::new(),
    })
}

pub(super) struct DecoderPermit {
    threads: usize,
}

impl DecoderPermit {
    pub(super) fn acquire(requested: u32, cancelled: &AtomicBool) -> AppResult<Self> {
        let budget = budget();
        let threads = (requested as usize).clamp(1, budget.limit);
        let mut used = budget.used.lock();
        while *used + threads > budget.limit && !cancelled.load(Ordering::Acquire) {
            budget.changed.wait(&mut used);
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        *used += threads;
        drop(used);
        log::debug!(target: "hongguo_playback::decoder", "threadBudget availableThreads={} requestedThreads={requested} grantedThreads={threads}", budget.limit);
        Ok(Self { threads })
    }

    pub(super) fn threads(&self) -> u32 {
        self.threads as u32
    }
}

impl Drop for DecoderPermit {
    fn drop(&mut self) {
        let budget = budget();
        *budget.used.lock() -= self.threads;
        budget.changed.notify_all();
    }
}

pub(super) fn wake() {
    budget().changed.notify_all();
}
