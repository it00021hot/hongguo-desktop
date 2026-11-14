use std::time::Instant;

use hongguo_common::{AppError, AppResult};

pub struct MonotonicClock {
    anchor_secs: f64,
    anchor: Instant,
    rate: f64,
    running: bool,
}

impl MonotonicClock {
    pub fn new(position_secs: f64, rate: f64, now: Instant) -> AppResult<Self> {
        validate(position_secs, rate)?;
        Ok(Self {
            anchor_secs: position_secs,
            anchor: now,
            rate,
            running: false,
        })
    }

    pub fn position(&self, now: Instant) -> f64 {
        self.anchor_secs
            + if self.running {
                now.saturating_duration_since(self.anchor).as_secs_f64() * self.rate
            } else {
                0.0
            }
    }

    pub fn set_running(&mut self, running: bool, now: Instant) {
        self.anchor_secs = self.position(now);
        self.anchor = now;
        self.running = running;
    }

    pub fn seek(&mut self, position_secs: f64, now: Instant) -> AppResult<()> {
        validate(position_secs, self.rate)?;
        self.anchor_secs = position_secs;
        self.anchor = now;
        Ok(())
    }

    pub fn set_rate(&mut self, rate: f64, now: Instant) -> AppResult<()> {
        validate(self.anchor_secs, rate)?;
        self.anchor_secs = self.position(now);
        self.anchor = now;
        self.rate = rate;
        Ok(())
    }
}

fn validate(position_secs: f64, rate: f64) -> AppResult<()> {
    if !position_secs.is_finite() || position_secs < 0.0 || !rate.is_finite() || rate <= 0.0 {
        return Err(AppError::Media(
            "invalid playback clock position or rate".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn silent_clock_preserves_pause_seek_and_rate_anchors() {
        let now = Instant::now();
        let mut clock = MonotonicClock::new(4.0, 1.0, now).unwrap();
        assert_eq!(clock.position(now + Duration::from_secs(2)), 4.0);
        clock.set_running(true, now);
        clock.set_rate(2.0, now + Duration::from_secs(1)).unwrap();
        assert_eq!(clock.position(now + Duration::from_secs(2)), 7.0);
        clock.set_running(false, now + Duration::from_secs(2));
        assert_eq!(clock.position(now + Duration::from_secs(20)), 7.0);
        clock.seek(10.0, now + Duration::from_secs(3)).unwrap();
        assert_eq!(clock.position(now + Duration::from_secs(20)), 10.0);
        assert!(clock.set_rate(f64::NAN, now).is_err());
    }
}
