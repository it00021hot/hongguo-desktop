//! 桌面没有 TTVideoEngine predictor，以真实 CDN Range 耗时提供网络策略输入。
//! 缓存读取不计入，过期样本视为未知，不用虚构带宽决定清晰度。
use parking_lot::Mutex;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

static SAMPLE: LazyLock<Mutex<Option<(Instant, f64)>>> = LazyLock::new(|| Mutex::new(None));

pub fn record_transfer(bytes: usize, elapsed: Duration) {
    if bytes < 16 * 1024 || elapsed.is_zero() {
        return;
    }
    let bps = bytes as f64 * 8.0 / elapsed.as_secs_f64();
    *SAMPLE.lock() = Some((Instant::now(), bps));
}

pub fn network_speed() -> f64 {
    SAMPLE
        .lock()
        .as_ref()
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(30))
        .map_or(0.0, |(_, speed)| *speed)
}
