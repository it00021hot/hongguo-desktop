use hongguo_common::AppResult;
use hongguo_video::sample_table::TrackInfo;
use std::sync::Mutex;

const RESERVE_SECONDS: f64 = 5.0;

struct Sample {
    start: u64,
    end: u64,
    seconds: f64,
}

pub(super) struct BufferingTrack {
    samples: Vec<Sample>,
    offsets: Vec<usize>,
    ready: Mutex<Option<(u64, u64, f64)>>,
}

impl BufferingTrack {
    pub(super) fn new(track: &TrackInfo) -> AppResult<Self> {
        let timeline = track.media_sample_timeline()?;
        let samples = track
            .samples
            .iter()
            .zip(timeline)
            .map(|(&(start, count), time)| Sample {
                start,
                end: start.saturating_add(count),
                seconds: time.dts as f64 / f64::from(track.media_timescale),
            })
            .collect::<Vec<_>>();
        let mut offsets = (0..samples.len()).collect::<Vec<_>>();
        offsets.sort_unstable_by_key(|&index| samples[index].start);
        Ok(Self {
            samples,
            offsets,
            ready: Mutex::new(None),
        })
    }

    pub(super) fn reserve(&self, start: u64, end: u64, rate: f64) -> Option<(u64, u64)> {
        if self
            .ready
            .lock()
            .expect("compressed buffer coverage")
            .is_some_and(|(first, last, buffered_rate)| {
                buffered_rate == rate && first <= start && end <= last
            })
        {
            return None;
        }
        let first = self
            .offsets
            .partition_point(|&index| self.samples[index].end <= start);
        let &index = self.offsets.get(first)?;
        let sample = &self.samples[index];
        if sample.start >= end {
            return None;
        }
        let until = sample.seconds + RESERVE_SECONDS * rate;
        Some(
            self.samples[index..]
                .iter()
                .take_while(|sample| sample.seconds < until)
                .fold((sample.start, sample.end), |(start, end), sample| {
                    (start.min(sample.start), end.max(sample.end))
                }),
        )
    }

    pub(super) fn mark_ready(&self, start: u64, end: u64, rate: f64) {
        *self.ready.lock().expect("compressed buffer coverage") = Some((start, end, rate));
    }
}
