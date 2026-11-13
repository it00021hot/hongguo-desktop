use std::time::Duration;

const COOLDOWN: Duration = Duration::from_millis(400);
const WHEEL_THRESHOLD: f64 = 15.0;
const TOUCH_THRESHOLD: f32 = 60.0;
const VERTICAL_RATIO: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Previous,
    Next,
}

impl Direction {
    pub fn step(self) -> i32 {
        match self {
            Self::Previous => -1,
            Self::Next => 1,
        }
    }
}

pub(super) fn wheel(delta: f64) -> Option<Direction> {
    if !delta.is_finite() || delta.abs() < WHEEL_THRESHOLD {
        return None;
    }
    Some(if delta > 0.0 {
        Direction::Next
    } else {
        Direction::Previous
    })
}

pub(super) fn swipe(dx: f32, dy: f32) -> Option<Direction> {
    if !dx.is_finite()
        || !dy.is_finite()
        || dy.abs() < TOUCH_THRESHOLD
        || dy.abs() < dx.abs() * VERTICAL_RATIO
    {
        return None;
    }
    Some(if dy < 0.0 {
        Direction::Next
    } else {
        Direction::Previous
    })
}

#[derive(Default)]
pub(super) struct Gate {
    last: Option<Duration>,
}

impl Gate {
    pub fn admit(&mut self, now: Duration) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_sub(last) < COOLDOWN)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_thresholds_preserve_horizontal_system_gestures() {
        assert_eq!(wheel(14.99), None);
        assert_eq!(wheel(15.0), Some(Direction::Next));
        assert_eq!(wheel(-15.0), Some(Direction::Previous));
        assert_eq!(swipe(0.0, -59.9), None);
        assert_eq!(swipe(40.0, -60.0), Some(Direction::Next));
        assert_eq!(swipe(40.01, -60.0), None);
        assert_eq!(swipe(-40.0, 60.0), Some(Direction::Previous));
        assert_eq!(swipe(80.0, 70.0), None);
        assert_eq!(wheel(f64::NAN), None);
        assert_eq!(swipe(f32::INFINITY, 80.0), None);
    }

    #[test]
    fn wheel_and_touch_share_one_lock_and_the_boundary_is_inclusive() {
        let mut gate = Gate::default();
        assert!(gate.admit(Duration::ZERO));
        assert!(!gate.admit(Duration::from_millis(399)));
        assert!(gate.admit(Duration::from_millis(400)));
        assert!(!gate.admit(Duration::from_millis(400)));
        assert!(gate.admit(Duration::from_millis(800)));
    }
}
