use std::{sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub struct PresentationObservation {
    pub source_id: String,
    pub revision: u64,
    pub generation: Option<u64>,
    pub serial: Option<u64>,
    pub pts_secs: Option<f64>,
    /// Metal host time in seconds; zero means the drawable was not displayed.
    pub presented_time_secs: f64,
    /// Callback host time in the same CACurrentMediaTime domain.
    pub callback_time_secs: f64,
    /// CPU-observed interval until native queue completion, not a GPU timestamp.
    pub gpu_elapsed: Duration,
}

pub(crate) type PresentationObserver = Arc<dyn Fn(PresentationObservation) + Send + Sync>;

pub(crate) struct PresentationContext {
    pub source_id: String,
    pub revision: u64,
    pub generation: Option<u64>,
    pub serial: Option<u64>,
    pub pts_secs: Option<f64>,
}

#[derive(Clone, Copy)]
pub(crate) struct PresentationTiming {
    pub presented_time_secs: f64,
    pub callback_time_secs: f64,
}

impl PresentationContext {
    pub fn observe(
        self,
        timing: PresentationTiming,
        gpu_elapsed: Duration,
    ) -> PresentationObservation {
        PresentationObservation {
            source_id: self.source_id,
            revision: self.revision,
            generation: self.generation,
            serial: self.serial,
            pts_secs: self.pts_secs,
            presented_time_secs: timing.presented_time_secs,
            callback_time_secs: timing.callback_time_secs,
            gpu_elapsed,
        }
    }
}

pub(crate) fn current_media_time() -> f64 {
    unsafe { CACurrentMediaTime() }
}

unsafe extern "C" {
    fn CACurrentMediaTime() -> f64;
}
