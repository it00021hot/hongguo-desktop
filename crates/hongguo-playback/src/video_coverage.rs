use hongguo_common::{AppError, AppResult};
use hongguo_video::timing::MediaSampleTime;

/// Union of presentation intervals; decode order can differ from PTS order.
pub(super) struct Coverage(Vec<(i64, i64)>);

impl Coverage {
    pub(super) fn new(samples: &[MediaSampleTime]) -> AppResult<Self> {
        let mut spans = samples
            .iter()
            .map(|sample| {
                sample
                    .pts
                    .checked_add(i64::from(sample.duration))
                    .map(|end| (sample.pts, end))
                    .ok_or_else(|| AppError::Media("sample presentation end overflow".into()))
            })
            .collect::<AppResult<Vec<_>>>()?;
        spans.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (start, end) in spans {
            if let Some(previous) = merged.last_mut()
                && start <= previous.1
            {
                previous.1 = previous.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        Ok(Self(merged))
    }

    pub(super) fn contains(&self, ticks: f64) -> bool {
        if !ticks.is_finite() {
            return false;
        }
        self.0
            .get(
                self.0
                    .partition_point(|&(start, _)| start as f64 <= ticks)
                    .wrapping_sub(1),
            )
            .is_some_and(|&(_, end)| ticks < end as f64)
    }
}
