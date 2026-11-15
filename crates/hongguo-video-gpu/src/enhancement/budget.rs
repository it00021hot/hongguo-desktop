use std::time::Duration;

const SLOW_FRAME: Duration = Duration::from_millis(150);
const SLOW_FRAME_LIMIT: u8 = 5;

#[derive(Default)]
pub struct FrameBudget {
    consecutive: u8,
    warmup_used: bool,
}

impl FrameBudget {
    pub fn observe(&mut self, elapsed: Duration) -> bool {
        self.consecutive = if elapsed > SLOW_FRAME {
            self.consecutive.saturating_add(1)
        } else {
            0
        };
        if self.consecutive < SLOW_FRAME_LIMIT {
            return false;
        }
        if self.warmup_used {
            return true;
        }
        self.warmup_used = true;
        self.consecutive = 0;
        false
    }
}
