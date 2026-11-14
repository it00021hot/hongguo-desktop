use hongguo_common::{AppError, AppResult};

use crate::sample_table::{TrackInfo, collect_tracks};

const RATE_SCALE: f64 = 65_536.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditSegment {
    pub movie_start_secs: f64,
    pub duration_secs: f64,
    pub media_start_secs: Option<f64>,
    pub media_rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MappedTime {
    pub edit_index: usize,
    pub movie_secs: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaPosition {
    pub edit_index: usize,
    pub media_secs: f64,
    pub media_rate: f64,
}

#[derive(Clone, Debug)]
pub struct TrackTimeline {
    pub track_id: u32,
    pub media_timescale: u32,
    segments: Vec<EditSegment>,
}

#[derive(Clone, Debug)]
pub struct MovieTimeline {
    pub timescale: u32,
    tracks: Vec<TrackTimeline>,
}

impl MovieTimeline {
    pub fn from_moov(data: &[u8]) -> AppResult<Self> {
        Self::from_tracks(super::movie_timescale(data)?, &collect_tracks(data)?)
    }

    pub fn from_tracks(timescale: u32, tracks: &[TrackInfo]) -> AppResult<Self> {
        if timescale == 0 {
            return Err(AppError::Media("movie timescale is zero".into()));
        }
        let tracks = tracks
            .iter()
            .map(|track| TrackTimeline::new(timescale, track))
            .collect::<AppResult<_>>()?;
        Ok(Self { timescale, tracks })
    }

    pub fn track(&self, track_id: u32) -> Option<&TrackTimeline> {
        self.tracks.iter().find(|track| track.track_id == track_id)
    }

    pub fn tracks(&self) -> &[TrackTimeline] {
        &self.tracks
    }
}

impl TrackTimeline {
    fn new(movie_timescale: u32, track: &TrackInfo) -> AppResult<Self> {
        if track.media_timescale == 0 {
            return Err(AppError::Media("media timescale is zero".into()));
        }
        let mut segments = Vec::with_capacity(track.edit_list.len().max(1));
        if track.edit_list.is_empty() {
            segments.push(EditSegment {
                movie_start_secs: 0.0,
                duration_secs: track.media_duration as f64 / f64::from(track.media_timescale),
                media_start_secs: Some(0.0),
                media_rate: 1.0,
            });
        }
        let mut movie_start = 0u64;
        for edit in &track.edit_list {
            if edit.media_time < -1 {
                return Err(AppError::Media("edit media_time is below -1".into()));
            }
            segments.push(EditSegment {
                movie_start_secs: movie_start as f64 / f64::from(movie_timescale),
                duration_secs: edit.segment_duration as f64 / f64::from(movie_timescale),
                media_start_secs: (edit.media_time >= 0)
                    .then_some(edit.media_time as f64 / f64::from(track.media_timescale)),
                media_rate: f64::from(edit.media_rate) / RATE_SCALE,
            });
            movie_start = movie_start
                .checked_add(edit.segment_duration)
                .ok_or_else(|| AppError::Media("edit movie duration overflow".into()))?;
        }
        Ok(Self {
            track_id: track.track_id,
            media_timescale: track.media_timescale,
            segments,
        })
    }

    pub fn segments(&self) -> &[EditSegment] {
        &self.segments
    }

    /// Repeated edits can present a media timestamp more than once. Empty edits never map media.
    pub fn media_to_movie(&self, media_secs: f64) -> Vec<MappedTime> {
        if !media_secs.is_finite() {
            return Vec::new();
        }
        self.segments
            .iter()
            .enumerate()
            .filter_map(|(edit_index, segment)| {
                let start = segment.media_start_secs?;
                let offset = if segment.media_rate == 0.0 {
                    if media_secs != start {
                        return None;
                    }
                    0.0
                } else {
                    (media_secs - start) / segment.media_rate
                };
                (offset >= 0.0 && offset < segment.duration_secs).then_some(MappedTime {
                    edit_index,
                    movie_secs: segment.movie_start_secs + offset,
                })
            })
            .collect()
    }

    /// Empty edits return None; dwell edits return a fixed media position with rate zero.
    pub fn movie_to_media(&self, movie_secs: f64) -> Option<MediaPosition> {
        if !movie_secs.is_finite() || movie_secs < 0.0 {
            return None;
        }
        self.segments
            .iter()
            .enumerate()
            .find_map(|(edit_index, segment)| {
                let offset = movie_secs - segment.movie_start_secs;
                if offset < 0.0 || offset >= segment.duration_secs {
                    return None;
                }
                Some(MediaPosition {
                    edit_index,
                    media_secs: segment.media_start_secs? + offset * segment.media_rate,
                    media_rate: segment.media_rate,
                })
            })
    }
}

#[cfg(test)]
#[path = "movie_timeline_tests.rs"]
mod tests;
