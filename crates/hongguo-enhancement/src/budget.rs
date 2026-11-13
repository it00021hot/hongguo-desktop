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

#[cfg(test)]
mod tests {
    use super::*;

    const SLOW: Duration = Duration::from_millis(151);
    const ACCEPTABLE: Duration = Duration::from_millis(150);

    #[test]
    fn one_warmup_burst_is_allowed_then_five_consecutive_slow_frames_stop() {
        let mut budget = FrameBudget::default();
        for _ in 0..9 {
            assert!(!budget.observe(SLOW));
        }
        assert!(budget.observe(SLOW));
    }

    #[test]
    fn normal_frames_break_consecutive_count_but_do_not_replenish_warmup() {
        let mut budget = FrameBudget::default();
        for _ in 0..5 {
            assert!(!budget.observe(SLOW));
        }
        for _ in 0..4 {
            assert!(!budget.observe(SLOW));
        }
        assert!(!budget.observe(ACCEPTABLE));
        for _ in 0..4 {
            assert!(!budget.observe(SLOW));
        }
        assert!(budget.observe(SLOW));
    }

    #[test]
    fn acceptable_boundary_never_stops_and_new_source_has_fresh_warmup() {
        let mut budget = FrameBudget::default();
        for _ in 0..20 {
            assert!(!budget.observe(ACCEPTABLE));
        }
        for _ in 0..9 {
            assert!(!budget.observe(SLOW));
        }
        assert!(budget.observe(SLOW));
        budget = FrameBudget::default();
        for _ in 0..5 {
            assert!(!budget.observe(SLOW));
        }
    }
}
