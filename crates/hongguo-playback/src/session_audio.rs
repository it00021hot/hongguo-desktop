use super::*;
use crate::{IndexedTrack, PcmChunk};
use hongguo_codec_ffi::{AudioDecoder, AudioFormat, PcmBlock};
use hongguo_video::timing::EditSegment;

const SILENCE_CHUNK_FRAMES: usize = 1024;

pub(super) fn run(shared: Arc<Shared>) {
    let mut previous = 0;
    let mut decoder = None;
    while let Some(job) = shared.job(previous) {
        previous = job.epoch;
        let result = job.index.audio.as_ref().map_or(Ok(()), |track| {
            decode_job(&shared, &job, track, &mut decoder)
        });
        match result {
            Ok(()) => {
                let mut state = shared.state.lock();
                if state.epoch == job.epoch {
                    state.audio_done = true;
                    shared.changed.notify_all();
                }
            }
            Err(_) if job.cancel.load(Ordering::Acquire) => {}
            Err(error) => shared.fail(error),
        }
    }
}

fn decode_job(
    shared: &Shared,
    job: &Job,
    track: &IndexedTrack,
    persistent: &mut Option<AudioDecoder>,
) -> AppResult<()> {
    let mut next_movie = job.target_secs;
    for segment in track.timeline.segments() {
        let end = segment.movie_start_secs + segment.duration_secs;
        if end <= job.target_secs {
            continue;
        }
        if segment.media_start_secs.is_none() || segment.media_rate == 0.0 {
            silence(shared, job, next_movie, end)?;
            next_movie = end;
            continue;
        }
        if segment.media_rate < 0.0 {
            return Err(AppError::Media(
                "reverse audio edits are unsupported".into(),
            ));
        }
        let output = AudioFormat {
            sample_rate: shared.config.pcm_format.sample_rate,
            channels: shared.config.pcm_format.channels,
        };
        let rate = job.rate * segment.media_rate;
        let media_target = segment.media_start_secs.unwrap()
            + (job.target_secs.max(segment.movie_start_secs) - segment.movie_start_secs)
                * segment.media_rate;
        let start = track.audio_seek_sample(media_target, rate, output.sample_rate);
        if persistent.is_none() {
            let mut decoder =
                AudioDecoder::new(&track.extradata, track.info.media_timescale, output, rate)
                    .map_err(codec_error)?;
            if start > 0 {
                // Read the stream's initial configuration access units once. AAC
                // flush preserves SBR/PS headers; subsequent seeks reuse that same
                // native context and only reset overlap/output-filter state.
                let seek = track.aac_seek.as_ref().expect("AAC seek metadata");
                log::debug!(
                    "[playback] bootstrap AAC object_type={} units={}",
                    seek.object_type,
                    seek.bootstrap_samples
                );
                let mut bytes = Vec::new();
                for index in 0..seek.bootstrap_samples.min(track.samples.len()) {
                    let (offset, length) = track.info.samples[index];
                    shared
                        .source
                        .read_at(offset, length, &job.cancel, &mut bytes)?;
                    while !decoder
                        .send(&bytes, Some(track.samples[index].pts))
                        .map_err(codec_error)?
                    {
                        drain(&mut decoder, &mut |_| Ok(()))?;
                    }
                    drain(&mut decoder, &mut |_| Ok(()))?;
                }
            }
            *persistent = Some(decoder);
        }
        let decoder = persistent.as_mut().expect("AAC context initialized");
        decoder.reset(rate).map_err(codec_error)?;
        let mut publish = |block| publish(shared, job, segment, block, &mut next_movie);
        let mut bytes = Vec::new();
        let media_end =
            segment.media_start_secs.unwrap() + segment.duration_secs * segment.media_rate;
        // Normative overlap/SBR pre-roll, container roll/prol, and the actual
        // atempo window history are decoded before trimming presentation.
        for (index, &(offset, length)) in track.info.samples.iter().enumerate().skip(start) {
            if track.samples[index].pts as f64 / f64::from(track.info.media_timescale) >= media_end
            {
                break;
            }
            shared
                .source
                .read_at(offset, length, &job.cancel, &mut bytes)?;
            if job.cancel.load(Ordering::Acquire) {
                return Err(AppError::Cancelled);
            }
            while !decoder
                .send(&bytes, Some(track.samples[index].pts))
                .map_err(codec_error)?
            {
                drain(decoder, &mut publish)?;
            }
            drain(decoder, &mut publish)?;
        }
        while !decoder.drain().map_err(codec_error)? {
            drain(decoder, &mut publish)?;
        }
        drain(decoder, &mut publish)?;
        silence(shared, job, next_movie, end)?;
        next_movie = end;
    }
    silence(shared, job, next_movie, job.index.duration_secs)
}

fn drain(
    decoder: &mut AudioDecoder,
    publish: &mut impl FnMut(PcmBlock) -> AppResult<()>,
) -> AppResult<()> {
    while let Some(block) = decoder.receive().map_err(codec_error)? {
        publish(block)?;
    }
    Ok(())
}

fn publish(
    shared: &Shared,
    job: &Job,
    segment: &EditSegment,
    block: PcmBlock,
    next_movie: &mut f64,
) -> AppResult<()> {
    let channels = usize::from(block.format.channels);
    if channels != usize::from(shared.config.pcm_format.channels)
        || block.format.sample_rate != shared.config.pcm_format.sample_rate
        || channels == 0
        || !block.samples.len().is_multiple_of(channels)
    {
        return Err(AppError::Media(
            "AAC PCM format differs from the negotiated output".into(),
        ));
    }
    let frames = block.samples.len() / channels;
    if frames == 0 {
        return Ok(());
    }
    let media_start = block
        .media_start_secs
        .ok_or_else(|| AppError::Media("AAC PCM has no source timestamp".into()))?;
    let movie_start = segment.movie_start_secs
        + (media_start - segment.media_start_secs.unwrap()) / segment.media_rate;
    let duration = block.media_duration_secs / segment.media_rate;
    if !duration.is_finite() || duration <= 0.0 {
        return Err(AppError::Media("AAC PCM has invalid duration".into()));
    }
    let begin = job
        .target_secs
        .max(segment.movie_start_secs)
        .max(*next_movie);
    let end = segment.movie_start_secs + segment.duration_secs;
    let first = (((begin - movie_start) / duration * frames as f64)
        .ceil()
        .max(0.0) as usize)
        .min(frames);
    let last = (((end - movie_start) / duration * frames as f64)
        .ceil()
        .max(0.0) as usize)
        .min(frames);
    if first >= last {
        return Ok(());
    }
    let start = movie_start + first as f64 * duration / frames as f64;
    if start > *next_movie {
        silence(shared, job, *next_movie, start)?;
    }
    let duration = ((last - first) as f64 * duration / frames as f64).min(end - start);
    let samples = block.samples[first * channels..last * channels].to_vec();
    shared.pcm.push(
        job.epoch,
        PcmChunk {
            samples,
            media_start_secs: start,
            media_duration_secs: duration,
        },
        &job.cancel,
    )?;
    *next_movie = start + duration;
    Ok(())
}

fn silence(shared: &Shared, job: &Job, start: f64, end: f64) -> AppResult<()> {
    let mut position = start.max(job.target_secs);
    let format = shared.config.pcm_format;
    let seconds_per_frame = job.rate / f64::from(format.sample_rate);
    let chunk_limit = SILENCE_CHUNK_FRAMES.min(shared.pcm.capacity_frames());
    while position < end {
        let frames = (((end - position) / seconds_per_frame).ceil() as usize).min(chunk_limit);
        if frames == 0 {
            break;
        }
        let duration = (frames as f64 * seconds_per_frame).min(end - position);
        shared.pcm.push(
            job.epoch,
            PcmChunk {
                samples: vec![0.0; frames * usize::from(format.channels)],
                media_start_secs: position,
                media_duration_secs: duration,
            },
            &job.cancel,
        )?;
        position += duration;
    }
    Ok(())
}

fn codec_error(error: hongguo_codec_ffi::CodecError) -> AppError {
    AppError::Media(error.to_string())
}
