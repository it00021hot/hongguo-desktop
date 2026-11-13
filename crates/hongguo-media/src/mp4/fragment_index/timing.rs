use hongguo_common::{AppError, AppResult};
use hongguo_video::{r#box::parse_boxes, sample_table::collect_tracks};

const DATA_OFFSET: u32 = 0x000001;
const FIRST_FLAGS: u32 = 0x000004;
const SAMPLE_DURATION: u32 = 0x000100;
const SAMPLE_SIZE: u32 = 0x000200;
const SAMPLE_FLAGS: u32 = 0x000400;
const COMPOSITION_OFFSET: u32 = 0x000800;
const NON_SYNC: u32 = 0x00010000;

pub(super) struct VideoRun {
    pub duration_ticks: u64,
    pub starts_with_sync: bool,
}

pub(super) fn video_track(moov: &[u8]) -> AppResult<(u32, u32)> {
    let track = collect_tracks(moov)?
        .into_iter()
        .find(|track| track.is_video && track.media_timescale > 0)
        .ok_or_else(|| {
            AppError::Media("fragment initialization has no timed video track".into())
        })?;
    Ok((track.track_id, track.media_timescale))
}

pub(super) fn video_run(moof: &[u8], header_len: u64, track_id: u32) -> AppResult<VideoRun> {
    for traf in parse_boxes(moof, header_len as usize, moof.len())
        .into_iter()
        .filter(|header| header.is("traf"))
    {
        let payload = &moof[traf.start..traf.start + traf.size];
        let boxes = parse_boxes(payload, 0, payload.len());
        let tfhd = boxes
            .iter()
            .find(|header| header.is("tfhd"))
            .ok_or_else(|| AppError::Media("fragment has no track header".into()))?;
        if u32_at(&payload[tfhd.start..tfhd.start + tfhd.size], 4)? != track_id {
            continue;
        }
        let mut result = None::<VideoRun>;
        for trun in boxes.iter().filter(|header| header.is("trun")) {
            let run = timing(&payload[trun.start..trun.start + trun.size])?;
            if let Some(previous) = &mut result {
                previous.duration_ticks =
                    previous
                        .duration_ticks
                        .checked_add(run.duration_ticks)
                        .ok_or_else(|| AppError::Media("fragment duration overflow".into()))?;
            } else {
                result = Some(run);
            }
        }
        return result.ok_or_else(|| AppError::Media("video fragment has no sample run".into()));
    }
    Err(AppError::Media("fragment has no video samples".into()))
}

fn timing(data: &[u8]) -> AppResult<VideoRun> {
    let flags = u32_at(data, 0)? & 0x00ff_ffff;
    let count = u32_at(data, 4)? as usize;
    if count == 0 || flags & SAMPLE_DURATION == 0 {
        return Err(AppError::Media(
            "fragment has no explicit sample durations".into(),
        ));
    }
    let mut offset = 8 + usize::from(flags & DATA_OFFSET != 0) * 4;
    let first = if flags & FIRST_FLAGS != 0 {
        let first = u32_at(data, offset)?;
        offset += 4;
        Some(first)
    } else {
        None
    };
    if flags & SAMPLE_FLAGS == 0 && first.is_none() {
        return Err(AppError::Media(
            "fragment has no explicit sync flags".into(),
        ));
    }
    let stride = [
        SAMPLE_DURATION,
        SAMPLE_SIZE,
        SAMPLE_FLAGS,
        COMPOSITION_OFFSET,
    ]
    .into_iter()
    .filter(|bit| flags & bit != 0)
    .count()
        * 4;
    if count > data.len().saturating_sub(offset) / stride {
        return Err(AppError::Media("fragment sample run is truncated".into()));
    }
    let mut duration = 0_u64;
    let mut sync = false;
    for index in 0..count {
        duration += u64::from(u32_at(data, offset)?);
        let flags_at = offset + 4 + usize::from(flags & SAMPLE_SIZE != 0) * 4;
        if index == 0 {
            sync = first.unwrap_or(if flags & SAMPLE_FLAGS != 0 {
                u32_at(data, flags_at)?
            } else {
                0
            }) & NON_SYNC
                == 0;
        }
        offset += stride;
    }
    if duration == 0 {
        return Err(AppError::Media("fragment duration is zero".into()));
    }
    Ok(VideoRun {
        duration_ticks: duration,
        starts_with_sync: sync,
    })
}

fn u32_at(data: &[u8], at: usize) -> AppResult<u32> {
    let bytes = data
        .get(at..at + 4)
        .ok_or_else(|| AppError::Media("fragment metadata field is truncated".into()))?;
    Ok(u32::from_be_bytes(bytes.try_into().expect("32-bit field")))
}
