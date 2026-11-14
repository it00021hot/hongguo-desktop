use std::collections::HashMap;

use hongguo_common::{AppError, AppResult};
use hongguo_media_types::ColorMetadata;
use hongguo_video::{
    r#box::{find_box, parse_boxes},
    sample_table::{TrackInfo, collect_tracks},
    timing::{MediaSampleTime, MovieTimeline, TrackTimeline},
};

const VISUAL_SAMPLE_ENTRY_BYTES: usize = 78;
const MAX_COMPRESSED_SAMPLE_BYTES: u64 = 64 * 1024 * 1024;

#[path = "video_coverage.rs"]
mod coverage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackCodec {
    Hevc,
    H264,
    Bytevc2,
    Aac,
}

pub struct IndexedTrack {
    pub info: TrackInfo,
    pub codec: TrackCodec,
    pub extradata: Vec<u8>,
    pub samples: Vec<MediaSampleTime>,
    pub timeline: TrackTimeline,
    pub color: ColorMetadata,
    pub rotation_degrees: u16,
    durations: HashMap<i64, u32>,
    coverage: coverage::Coverage,
    pub(crate) aac_seek: Option<crate::aac_seek::AacSeek>,
}

pub struct IndexedSource {
    pub video: IndexedTrack,
    pub audio: Option<IndexedTrack>,
    pub duration_secs: f64,
}

impl IndexedSource {
    pub fn open(
        length: u64,
        read: impl FnMut(u64, u64, &mut Vec<u8>) -> AppResult<()>,
    ) -> AppResult<Self> {
        let (_, moov) = hongguo_bytevc2::read_moov_at(length, read)?;
        let source = Self::from_moov(&moov)?;
        for track in std::iter::once(&source.video).chain(source.audio.as_ref()) {
            if track
                .info
                .samples
                .iter()
                .any(|&(offset, size)| offset.checked_add(size).is_none_or(|end| end > length))
            {
                return Err(AppError::Media(
                    "sample table points outside the source".into(),
                ));
            }
        }
        Ok(source)
    }

    pub fn from_moov(moov: &[u8]) -> AppResult<Self> {
        let tracks = collect_tracks(moov)?;
        let movie =
            MovieTimeline::from_tracks(hongguo_video::timing::movie_timescale(moov)?, &tracks)?;
        let mut video = None;
        let mut audio = None;
        for info in tracks {
            if info.is_video && video.is_none() {
                video = Some(IndexedTrack::new(moov, info, &movie)?);
            } else if !info.is_video && info.sample_rate > 0 && audio.is_none() {
                audio = Some(IndexedTrack::new(moov, info, &movie)?);
            }
        }
        let video = video.ok_or_else(|| AppError::Media("container has no video track".into()))?;
        let duration_secs = std::iter::once(&video)
            .chain(audio.as_ref())
            .flat_map(|track| track.timeline.segments())
            .map(|segment| segment.movie_start_secs + segment.duration_secs)
            .fold(0.0, f64::max);
        Ok(Self {
            video,
            audio,
            duration_secs,
        })
    }
}

impl IndexedTrack {
    fn new(moov: &[u8], info: TrackInfo, movie: &MovieTimeline) -> AppResult<Self> {
        if info
            .samples
            .iter()
            .any(|sample| sample.1 > MAX_COMPRESSED_SAMPLE_BYTES)
        {
            return Err(AppError::Media(
                "compressed sample exceeds playback byte budget".into(),
            ));
        }
        let codec = match info.codec.as_str() {
            "hvc1" | "hev1" => TrackCodec::Hevc,
            "avc1" | "avc3" => TrackCodec::H264,
            "bvc2" => TrackCodec::Bytevc2,
            "mp4a" => TrackCodec::Aac,
            other => {
                return Err(AppError::Media(format!(
                    "unsupported playback codec: {other}"
                )));
            }
        };
        let (extradata, color) = if codec == TrackCodec::Aac {
            (info.decoder_config.clone(), ColorMetadata::default())
        } else {
            visual_configuration(moov, &info, codec)?
        };
        if extradata.is_empty() {
            return Err(AppError::Media("track has no decoder configuration".into()));
        }
        let samples = info.media_sample_timeline()?;
        let coverage = coverage::Coverage::new(&samples)?;
        let durations = samples
            .iter()
            .map(|sample| (sample.pts, sample.duration))
            .collect();
        let timeline = movie
            .track(info.track_id)
            .cloned()
            .ok_or_else(|| AppError::Media("track has no movie timeline".into()))?;
        let rotation_degrees = track_rotation(moov, info.track_id)?;
        let aac_seek = if codec == TrackCodec::Aac {
            Some(crate::aac_seek::AacSeek::new(
                &extradata,
                &moov[info.stbl_offset..info.stbl_offset + info.stbl_size],
            )?)
        } else {
            None
        };
        Ok(Self {
            info,
            codec,
            extradata,
            samples,
            timeline,
            color,
            rotation_degrees,
            durations,
            coverage,
            aac_seek,
        })
    }

    pub fn seek_sample(&self, media_secs: f64) -> usize {
        let target = media_secs * f64::from(self.info.media_timescale);
        let is_sync = |index: usize| {
            self.info
                .stss
                .as_ref()
                .is_none_or(|sync| sync.binary_search(&(index as u32 + 1)).is_ok())
        };
        self.samples
            .iter()
            .enumerate()
            .filter(|(index, sample)| is_sync(*index) && sample.pts as f64 <= target)
            .max_by_key(|(_, sample)| sample.pts)
            .map_or(0, |(index, _)| index)
    }

    pub fn audio_seek_sample(&self, media_secs: f64, rate: f64, output_rate: u32) -> usize {
        let Some(seek) = &self.aac_seek else {
            return 0;
        };
        let target_ticks = media_secs * f64::from(self.info.media_timescale);
        let target = self
            .samples
            .partition_point(|sample| sample.pts as f64 <= target_ticks)
            .saturating_sub(1);
        let history = seek.history_secs(rate, output_rate);
        if !history.is_finite() {
            return 0;
        }
        let start_ticks = (media_secs - history) * f64::from(self.info.media_timescale);
        let codec_start = self
            .samples
            .partition_point(|sample| (sample.pts as f64) < start_ticks)
            .saturating_sub(1);
        seek.group_start(target)
            .map_or(codec_start, |group| group.min(codec_start))
    }

    pub fn duration_for_pts(&self, media_secs: f64) -> AppResult<f64> {
        let ticks = (media_secs * f64::from(self.info.media_timescale)).round() as i64;
        self.durations
            .get(&ticks)
            .map(|duration| f64::from(*duration) / f64::from(self.info.media_timescale))
            .ok_or_else(|| {
                AppError::Media("decoded frame timestamp is absent from sample timeline".into())
            })
    }

    pub(crate) fn expects_frame_at(&self, movie_secs: f64) -> bool {
        self.timeline
            .movie_to_media(movie_secs)
            .is_some_and(|time| {
                self.coverage
                    .contains(time.media_secs * f64::from(self.info.media_timescale))
            })
    }

    pub(crate) fn is_empty_edit_at(&self, movie_secs: f64) -> bool {
        self.timeline.segments().iter().any(|segment| {
            segment.media_start_secs.is_none()
                && movie_secs >= segment.movie_start_secs
                && movie_secs < segment.movie_start_secs + segment.duration_secs
        })
    }
}

fn visual_configuration(
    data: &[u8],
    info: &TrackInfo,
    codec: TrackCodec,
) -> AppResult<(Vec<u8>, ColorMetadata)> {
    let stsd = find_box(
        data,
        info.stbl_offset,
        info.stbl_offset + info.stbl_size,
        "stsd",
    )
    .ok_or_else(|| AppError::Media("video track has no stsd".into()))?;
    let entries = parse_boxes(data, stsd.start + 8, stsd.start + stsd.size);
    if entries.len() != 1 {
        return Err(AppError::Media(
            "multiple video sample descriptions are unsupported".into(),
        ));
    }
    let entry = &entries[0];
    let inner = entry.start + VISUAL_SAMPLE_ENTRY_BYTES;
    if inner > entry.start + entry.size {
        return Err(AppError::Media("truncated visual sample entry".into()));
    }
    let configuration_box = match codec {
        TrackCodec::Hevc => "hvcC",
        TrackCodec::H264 => "avcC",
        TrackCodec::Bytevc2 => "bv2C",
        TrackCodec::Aac => unreachable!(),
    };
    let configuration = find_box(data, inner, entry.start + entry.size, configuration_box)
        .ok_or_else(|| AppError::Media(format!("video track has no {configuration_box}")))?;
    let mut color = parse_color(data, inner, entry.start + entry.size);
    if let Some(pasp) = find_box(data, inner, entry.start + entry.size, "pasp")
        && pasp.size == 8
    {
        let ratio = |at| u32::from_be_bytes(data[at..at + 4].try_into().expect("four bytes"));
        let (numerator, denominator) = (ratio(pasp.start), ratio(pasp.start + 4));
        if numerator > 0 && denominator > 0 {
            color.sample_aspect_ratio = Some((numerator, denominator));
        }
    }
    Ok((
        data[configuration.start..configuration.start + configuration.size].to_vec(),
        color,
    ))
}

fn track_rotation(data: &[u8], track_id: u32) -> AppResult<u16> {
    let moov = find_box(data, 0, data.len(), "moov")
        .ok_or_else(|| AppError::Media("no moov for display matrix".into()))?;
    for track in parse_boxes(data, moov.start, moov.start + moov.size)
        .iter()
        .filter(|track| track.is("trak"))
    {
        let Some(tkhd) = find_box(data, track.start, track.start + track.size, "tkhd") else {
            continue;
        };
        let payload = &data[tkhd.start..tkhd.start + tkhd.size];
        let wide = payload.first() == Some(&1);
        let (id_at, matrix_at) = if wide { (20, 52) } else { (12, 40) };
        if payload.len() < matrix_at + 20 {
            return Err(AppError::Media("truncated track display matrix".into()));
        }
        let read = |at| i32::from_be_bytes(payload[at..at + 4].try_into().expect("four bytes"));
        if read(id_at) as u32 != track_id {
            continue;
        }
        const ONE: i32 = 65_536;
        const NEGATIVE_ONE: i32 = -ONE;
        return match (
            read(matrix_at),
            read(matrix_at + 4),
            read(matrix_at + 12),
            read(matrix_at + 16),
        ) {
            (ONE, 0, 0, ONE) => Ok(0),
            (0, ONE, NEGATIVE_ONE, 0) => Ok(90),
            (NEGATIVE_ONE, 0, 0, NEGATIVE_ONE) => Ok(180),
            (0, NEGATIVE_ONE, ONE, 0) => Ok(270),
            _ => Err(AppError::Media(
                "unsupported non-orthogonal track display matrix".into(),
            )),
        };
    }
    Ok(0)
}

fn parse_color(data: &[u8], start: usize, end: usize) -> ColorMetadata {
    let Some(colr) = find_box(data, start, end, "colr") else {
        return ColorMetadata::default();
    };
    let data = &data[colr.start..colr.start + colr.size];
    if data.len() < 10 || !matches!(&data[..4], b"nclx" | b"nclc") {
        return ColorMetadata::default();
    }
    let component = |at| u8::try_from(u16::from_be_bytes([data[at], data[at + 1]])).ok();
    ColorMetadata {
        primaries: component(4),
        transfer: component(6),
        matrix: component(8),
        full_range: (data.get(..4) == Some(b"nclx"))
            .then(|| data.get(10).map(|value| value & 0x80 != 0))
            .flatten(),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
