use std::time::{Duration, Instant};

const OCCLUSION_SCAN_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct ExposureRefresh {
    previous: Option<(bool, Instant)>,
}

impl ExposureRefresh {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    pub fn should_scan(&mut self, foreground: bool) -> bool {
        let now = Instant::now();
        // Window restoration or activation invalidates the expensive scan's
        // cooldown; steady-state occlusion checks stay rate-limited.
        if self.previous.is_some_and(|(was_foreground, at)| {
            was_foreground == foreground && now.duration_since(at) < OCCLUSION_SCAN_INTERVAL
        }) {
            return false;
        }
        self.previous = Some((foreground, now));
        true
    }
}
