use super::decoder::{Decoder, Failure};
use super::*;
use crate::{IndexedTrack, TrackCodec};
use hongguo_media_types::DecodedVideoFrame;
use hongguo_video::timing::EditSegment;

struct Job {
    epoch: u64,
    target: f64,
    cancel: Arc<AtomicBool>,
    index: Arc<IndexedSource>,
    device: Option<Arc<dyn DecodeDevice>>,
}

pub(super) fn run(shared: &Shared) -> AppResult<()> {
    let index = Arc::new(IndexedSource::open(
        shared.source.length()?,
        |offset, count, bytes| {
            shared
                .source
                .read_at(offset, count, &shared.metadata_cancel, bytes)
        },
    )?);
    if !matches!(index.video.codec, TrackCodec::Hevc | TrackCodec::Bytevc2) {
        return Err(AppError::Media(
            "this format belongs to native Video, not the codec extension".into(),
        ));
    }
    {
        let mut state = shared.state.lock();
        if state.closed {
            return Ok(());
        }
        state.index = Some(index.clone());
        shared.changed.notify_all();
    }
    let mut previous = 0;
    loop {
        let job = {
            let mut state = shared.state.lock();
            while !state.closed
                && state.error.is_none()
                && (state.epoch == previous || !state.decoder_bound)
            {
                shared.changed.wait(&mut state);
            }
            if state.closed || state.error.is_some() {
                return Ok(());
            }
            Job {
                epoch: state.epoch,
                target: state.target,
                cancel: state.cancel.clone(),
                index: index.clone(),
                device: if state.hardware_disabled {
                    None
                } else {
                    state.device.clone()
                },
            }
        };
        previous = job.epoch;
        match decode(shared, &job) {
            Ok(()) => {
                let mut state = shared.state.lock();
                if state.epoch == job.epoch {
                    state.video_done = true;
                    shared.changed.notify_all();
                }
            }
            Err(_) if job.cancel.load(Ordering::Acquire) => {}
            Err(Failure::Hardware(detail)) => {
                let mut state = shared.state.lock();
                if state.epoch != job.epoch {
                    continue;
                }
                shared.refresh(&mut state)?;
                state.hardware_disabled = true;
                let target = state.position;
                log::warn!(
                    "[video] HEVC hardware decode failed; restarting libde265 at {target:.3}s: {detail}"
                );
                shared.restart(&mut state, target)?;
            }
            Err(Failure::Media(error)) => return Err(error),
        }
    }
}

fn decode(shared: &Shared, job: &Job) -> Result<(), Failure> {
    let permit = crate::budget::DecoderPermit::acquire(shared.config.decoder_threads, &job.cancel)?;
    let track = &job.index.video;
    let mut device = job.device.as_deref();
    for segment in track.timeline.segments() {
        if segment.movie_start_secs + segment.duration_secs <= job.target {
            continue;
        }
        let Some(media_start) = segment.media_start_secs else {
            continue;
        };
        if segment.media_rate < 0.0 {
            return Err(AppError::Media("reverse video edits are unsupported".into()).into());
        }
        let target =
            media_start + (job.target - segment.movie_start_secs).max(0.0) * segment.media_rate;
        let start = track.seek_sample(target);
        let (parameters, length_size) = match track.codec {
            TrackCodec::Hevc => {
                if track.info.codec == "hev1"
                    && track.extradata.len() == 23
                    && track.extradata[22] == 0
                {
                    (Vec::new(), (track.extradata[21] & 3) as usize + 1)
                } else {
                    let (nalus, length) =
                        hongguo_media::hevc::param_nalus_from_hvcc(&track.extradata)?;
                    let parameters = nalus
                        .into_iter()
                        .flat_map(|nal| [0, 0, 0, 1].into_iter().chain(nal))
                        .collect();
                    (parameters, length)
                }
            }
            TrackCodec::Bytevc2 => (
                hongguo_bytevc2::annexb_from_extradata(&track.extradata)?,
                hongguo_bytevc2::length_size_from_extradata(&track.extradata)?,
            ),
            _ => return Err(AppError::Media("invalid extension video codec".into()).into()),
        };
        let mut decoder = None;
        let mut bytes = Vec::new();
        let mut complete = false;
        for (sample_index, &(offset, count)) in track.info.samples.iter().enumerate().skip(start) {
            if job.cancel.load(Ordering::Acquire) {
                return Err(AppError::Cancelled.into());
            }
            shared
                .source
                .read_at(offset, count, &job.cancel, &mut bytes)?;
            if bytes.len() as u64 != count {
                return Err(AppError::Media("short compressed video sample".into()).into());
            }
            let first = sample_index == start;
            let mut packet = if first {
                parameters.clone()
            } else {
                Vec::new()
            };
            packet.extend(hongguo_bytevc2::sample_to_annexb_checked(
                &bytes,
                length_size,
            )?);
            let time = track.samples[sample_index].pts;
            if decoder.is_none() {
                let (selected, unavailable) =
                    Decoder::new(track, permit.threads(), device, &packet, &job.cancel)?;
                decoder = Some(selected);
                if unavailable {
                    device = None;
                    let mut state = shared.state.lock();
                    if state.epoch == job.epoch {
                        state.hardware_disabled = true;
                    }
                }
            }
            for frame in decoder
                .as_mut()
                .expect("decoder initialized from first access unit")
                .decode(&packet, time, first)?
            {
                if !publish(shared, job, track, segment, frame)? {
                    complete = true;
                    break;
                }
            }
            if complete {
                break;
            }
        }
        if !complete && let Some(mut decoder) = decoder {
            for frame in decoder.drain()? {
                if !publish(shared, job, track, segment, frame)? {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn publish(
    shared: &Shared,
    job: &Job,
    track: &IndexedTrack,
    segment: &EditSegment,
    decoded: DecodedVideoFrame,
) -> AppResult<bool> {
    let source_pts = decoded.pts_secs;
    let source_duration = track.duration_for_pts(source_pts)?;
    let media_start = segment
        .media_start_secs
        .ok_or_else(|| AppError::Media("cannot publish an empty edit".into()))?;
    let (pts, duration) = if segment.media_rate == 0.0 {
        if source_pts + source_duration <= media_start {
            return Ok(true);
        }
        (segment.movie_start_secs, segment.duration_secs)
    } else {
        let pts = segment.movie_start_secs + (source_pts - media_start) / segment.media_rate;
        if pts >= segment.movie_start_secs + segment.duration_secs {
            return Ok(false);
        }
        let end = (pts + source_duration / segment.media_rate)
            .min(segment.movie_start_secs + segment.duration_secs);
        let pts = pts.max(segment.movie_start_secs);
        (pts, end - pts)
    };
    if duration <= 0.0 || pts + duration <= job.target {
        return Ok(true);
    }
    let mut descriptor = decoded.image.descriptor();
    descriptor.rotation_degrees = track.rotation_degrees;
    let mut color = track.color.clone();
    color.inherit(&descriptor.color);
    descriptor.color = color;
    let image = decoded
        .image
        .with_color_and_rotation(descriptor.color, descriptor.rotation_degrees);
    let bytes = match &image {
        hongguo_media_types::FrameImage::Cpu(frame) => {
            frame.planes.iter().map(|plane| plane.data.len()).sum()
        }
        hongguo_media_types::FrameImage::Hardware(frame) => {
            let descriptor = frame.descriptor();
            descriptor
                .width
                .saturating_mul(descriptor.height)
                .saturating_mul(3)
        }
    };
    if bytes > shared.config.video_queue_bytes {
        return Err(AppError::Media("video frame exceeds memory budget".into()));
    }
    let mut state = shared.state.lock();
    loop {
        state.check()?;
        if state.epoch != job.epoch || job.cancel.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        shared.refresh(&mut state)?;
        if state.queue.len() < shared.config.video_queue_frames
            && state
                .bytes
                .checked_add(bytes)
                .is_some_and(|total| total <= shared.config.video_queue_bytes)
            && (state.holding || pts - state.position <= shared.config.video_ahead_secs)
        {
            break;
        }
        shared.changed.wait(&mut state);
    }
    let serial = state.next_serial;
    state.next_serial = serial
        .checked_add(1)
        .ok_or_else(|| AppError::Media("video frame serial exhausted".into()))?;
    state.bytes += bytes;
    state.queue.push_back(Arc::new(PresentationFrame {
        serial,
        generation: job.epoch,
        pts_secs: pts,
        duration_secs: duration,
        image,
    }));
    shared.changed.notify_all();
    Ok(segment.media_rate != 0.0)
}
