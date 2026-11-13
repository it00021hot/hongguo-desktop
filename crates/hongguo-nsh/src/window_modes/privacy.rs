const PAUSE_GRACE_MS: u64 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Pause,
    Hide,
    Show,
}

#[derive(Default)]
pub(super) struct Privacy {
    pub enabled: bool,
    pub hidden: bool,
    hide_at: Option<u64>,
}

impl Privacy {
    pub fn sample(
        &mut self,
        now: u64,
        inside: bool,
        minimized: bool,
        suspended: bool,
    ) -> Vec<Effect> {
        if suspended || minimized {
            return Vec::new();
        }
        if !self.enabled || inside {
            if self.hidden || self.hide_at.take().is_some() {
                self.hidden = false;
                return vec![Effect::Show];
            }
            return Vec::new();
        }
        if self.hidden {
            return Vec::new();
        }
        if let Some(deadline) = self.hide_at {
            if now >= deadline {
                self.hide_at = None;
                self.hidden = true;
                return vec![Effect::Hide];
            }
            return Vec::new();
        }
        self.hide_at = Some(now + PAUSE_GRACE_MS);
        vec![Effect::Pause]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_leave_pauses_before_hiding_and_return_restores_in_place() {
        let mut p = Privacy {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(p.sample(0, false, false, false), vec![Effect::Pause]);
        assert!(!p.hidden);
        assert!(p.sample(79, false, false, false).is_empty());
        assert_eq!(p.sample(80, false, false, false), vec![Effect::Hide]);
        assert!(p.hidden);
        assert_eq!(p.sample(120, true, false, false), vec![Effect::Show]);
        assert!(!p.hidden);
    }
    #[test]
    fn cursor_return_cancels_a_pending_hide_and_disabled_never_steals_focus() {
        let mut p = Privacy {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(p.sample(0, false, false, false), vec![Effect::Pause]);
        assert_eq!(p.sample(30, true, false, false), vec![Effect::Show]);
        assert!(p.sample(120, true, false, false).is_empty());
        p.enabled = false;
        assert!(p.sample(240, false, false, false).is_empty());
    }
    #[test]
    fn minimized_and_boss_hidden_windows_are_never_revealed_by_cursor_samples() {
        let mut p = Privacy {
            enabled: true,
            ..Default::default()
        };
        assert!(p.sample(0, false, true, false).is_empty());
        assert!(p.sample(120, false, false, true).is_empty());
        assert_eq!(p.sample(240, false, false, false), vec![Effect::Pause]);
        assert_eq!(p.sample(320, false, false, false), vec![Effect::Hide]);
        assert!(p.sample(360, true, false, true).is_empty());
        assert!(p.hidden);
        p.enabled = false;
        assert_eq!(p.sample(480, false, false, false), vec![Effect::Show]);
    }
}
