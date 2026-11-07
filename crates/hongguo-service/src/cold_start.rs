//! 冷启动预热门闩：预热完成前，取流和起播在这里等。
//! 未 `arm` 时是开的，单测和还没进启动装配的调用不会被卡住。

use std::sync::atomic::{AtomicU8, Ordering};

use tokio::sync::Notify;

const OPEN: u8 = 0;
const PENDING: u8 = 1;
const READY: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(OPEN);
static NOTIFY: Notify = Notify::const_new();

/// 启动装配里、任何起播之前调用。已经在等或已经完成时不再退回。
pub fn arm() {
    let _ = STATE.compare_exchange(OPEN, PENDING, Ordering::AcqRel, Ordering::Acquire);
}

/// 预热结束（成功或注册失败放弃）后放行。
pub fn release() {
    STATE.store(READY, Ordering::Release);
    NOTIFY.notify_waiters();
}

/// 等到可以取流。门闩没武装时立刻返回。
pub async fn wait() {
    loop {
        let notified = NOTIFY.notified();
        tokio::pin!(notified);
        if STATE.load(Ordering::Acquire) != PENDING {
            return;
        }
        notified.await;
    }
}

/// `ensure_registered` 的全部出口都放行，包括提前返回和 panic。
pub struct ReleaseOnDrop;

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        release();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn unarmed_gate_does_not_block() {
        let started = Instant::now();
        super::wait().await;
        assert!(started.elapsed() < Duration::from_millis(200));
    }
}
