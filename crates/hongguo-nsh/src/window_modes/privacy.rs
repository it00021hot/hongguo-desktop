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
