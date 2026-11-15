#[derive(Debug, PartialEq)]
pub(super) struct DefinitionChange {
    pub definition: Option<u32>,
    pub resume_at: f64,
    pub reset_definition: bool,
}

pub(super) fn change_definition(
    current: Option<u32>,
    requested: Option<u32>,
    at: f64,
) -> Option<DefinitionChange> {
    (current != requested).then_some(DefinitionChange {
        definition: requested,
        reset_definition: requested.is_none(),
        resume_at: if at.is_finite() && at >= 0.0 { at } else { 0.0 },
    })
}

pub(super) fn change_episode(current: u32, requested: u32) -> bool {
    current != requested
}

pub(super) fn initial_seek(resume_at: f64) -> Option<f64> {
    (resume_at.is_finite() && resume_at > 0.0).then_some(resume_at)
}

pub(super) fn source_current(loading: bool, failed: bool, same_lease: bool) -> bool {
    !loading && !failed && same_lease
}

pub(super) fn seek_available(target: f64, native_duration: f64) -> bool {
    target.is_finite()
        && target >= 0.0
        && (!native_duration.is_finite() || native_duration <= 0.0 || target < native_duration)
}

#[derive(Default)]
pub(super) struct SeekLatch {
    issued: Option<f64>,
}

impl SeekLatch {
    pub fn dispatch(&mut self, target: f64) -> bool {
        if self.issued == Some(target) {
            return false;
        }
        self.issued = Some(target);
        true
    }

    pub fn confirmed(&self, target: f64, actual: f64) -> bool {
        const NATIVE_CONFIRMATION_TOLERANCE: f64 = 1.0;
        self.issued == Some(target)
            && actual.is_finite()
            && actual >= 0.0
            && (actual - target).abs() <= NATIVE_CONFIRMATION_TOLERANCE
    }

    pub fn clear(&mut self) {
        self.issued = None;
    }
}

#[derive(Default)]
pub(super) struct LoadLatch {
    loaded: bool,
}

impl LoadLatch {
    pub fn seek(&mut self, pending: Option<f64>, saved: f64, duration: f64) -> Option<f64> {
        if self.loaded {
            return None;
        }
        self.loaded = true;
        let at = pending.unwrap_or(saved);
        Some(
            if at.is_finite()
                && at >= 0.0
                && (!duration.is_finite() || duration <= 0.0 || at < duration)
            {
                at
            } else {
                0.0
            },
        )
    }
}
