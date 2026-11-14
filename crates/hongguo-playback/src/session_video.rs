use super::*;
use crate::{IndexedTrack, TrackCodec};
use hongguo_codec_ffi::{Codec, CodecError, DecoderConfig, Packet, VideoDecoder};
use hongguo_media_types::{
    ColorMetadata, FrameDescriptor, FrameImage, HardwareFrame, HardwareHandle, HardwareKind,
};

enum PipelineError {
    Source(AppError),
    Decoder(String),
}
impl From<AppError> for PipelineError {
    fn from(error: AppError) -> Self {
        Self::Source(error)
    }
}
impl From<CodecError> for PipelineError {
    fn from(error: CodecError) -> Self {
        // Hardware APIs can report INVALIDDATA for a backend/driver limitation. Only a
        // bounded software attempt on the same source can distinguish it from bad media.
        Self::Decoder(error.to_string())
    }
}

pub(super) fn run(shared: Arc<Shared>) {
    if let Err(error) = prepare(&shared) {
        shared.fail(error);
        return;
    }
    let mut previous = 0;
    while let Some(job) = shared.job(previous) {
        previous = job.epoch;
        let backend = select_backend(&shared, &job);
        match decode_job(&shared, &job, backend) {
            Ok(()) => {
                let mut state = shared.state.lock();
                if state.epoch == job.epoch {
                    state.video_done = true;
                    state.video_work = (buffering::VideoWork::Waiting, Instant::now());
                    shared.changed.notify_all();
                }
            }
            Err(error) => handle_failure(&shared, &job, backend, error),
        }
    }
}

fn handle_failure(shared: &Shared, job: &Job, backend: Backend, error: PipelineError) {
    if job.cancel.load(Ordering::Acquire) {
        return;
    }
    match error {
        PipelineError::Decoder(error) if backend != Backend::Software => {
            recover(shared, job, backend, error)
        }
        PipelineError::Decoder(error) => shared.fail(error),
        PipelineError::Source(error) => shared.fail(error),
    }
}

fn prepare(shared: &Shared) -> AppResult<()> {
    loop {
        let (epoch, cancel) = {
            let state = shared.state.lock();
            state.check()?;
            (state.epoch, state.cancel.clone())
        };
        let index = IndexedSource::open(shared.source.length()?, |offset, length, bytes| {
            shared.source.read_at(offset, length, &cancel, bytes)
        });
        let mut state = shared.state.lock();
        state.check()?;
        if state.epoch != epoch {
            continue;
        }
        let index = Arc::new(index?);
        state.stream_key = Some(StreamKey::new(&index.video));
        state.audio_done = index.audio.is_none();
        state.audio_output_pending = index.audio.is_some();
        if index.audio.is_some() && state.decoder_bound {
            let _ = shared.output.send(OutputCommand::Open);
        }
        state.index = Some(index);
        if state.index.as_ref().is_some_and(|index| {
            index
                .video
                .timeline
                .movie_to_media(state.target_secs)
                .is_none()
        }) {
            state.holding = false;
            let playing = state.desired_playing;
            state.mode = state.playback_mode();
            state.silent_clock.set_running(playing, Instant::now());
            if playing {
                let _ = shared.output.send(OutputCommand::Start);
            }
        }
        shared.changed.notify_all();
        return Ok(());
    }
}

fn select_backend(shared: &Shared, job: &Job) -> Backend {
    if job.index.video.codec == TrackCodec::Bytevc2 {
        return Backend::Software;
    }
    let Some(device) = &job.device else {
        return Backend::Software;
    };
    let backend = match device.kind() {
        HardwareKind::VideoToolbox => Backend::VideoToolbox,
        HardwareKind::D3d11 => Backend::D3d11 {
            device: device.device(),
        },
        HardwareKind::MediaCodec => Backend::MediaCodec {
            java_vm: device.device(),
            surface: device.surface(),
        },
    };
    if shared
        .state
        .lock()
        .failed
        .contains(&(backend, device.generation(), job.stream_key))
    {
        Backend::Software
    } else {
        backend
    }
}

pub(super) fn recover(shared: &Shared, job: &Job, backend: Backend, error: String) {
    let mut state = shared.state.lock();
    if shared.check_recovery_deadline(&mut state).is_err() {
        return;
    }
    if state.epoch != job.epoch || state.check().is_err() {
        return;
    }
    let position = shared.position_locked(&state);
    let stream_key = state.stream_key.unwrap_or(job.stream_key);
    state.failed.insert((
        backend,
        job.device.as_ref().map_or(0, |device| device.generation()),
        stream_key,
    ));
    if let Err(error) = shared.recover_restart(&mut state, position) {
        drop(state);
        shared.fail(error);
        return;
    }
    log::warn!(
        "playback backend {backend:?} failed; recovering the same source in software: {error}"
    );
}

fn decode_job(shared: &Shared, job: &Job, backend: Backend) -> Result<(), PipelineError> {
    let track = &job.index.video;
    for segment in track.timeline.segments() {
        if segment.movie_start_secs + segment.duration_secs < job.target_secs {
            continue;
        }
        let Some(media_start) = segment.media_start_secs else {
            continue;
        };
        if segment.media_rate < 0.0 {
            return Err(AppError::Media("reverse video edits are unsupported".into()).into());
        }
        let target = media_start
            + (job.target_secs - segment.movie_start_secs).max(0.0) * segment.media_rate;
        let start = track.seek_sample(target);
        let mut publish = |image, pts| publish(shared, job, track, segment, image, pts);
        if track.codec == TrackCodec::Bytevc2 {
            decode_vvdec(shared, job, track, start, &mut publish)?;
        } else {
            decode_ffmpeg(shared, job, backend, track, start, &mut publish)?;
        }
    }
    Ok(())
}

fn decode_ffmpeg(
    shared: &Shared,
    job: &Job,
    backend: Backend,
    track: &IndexedTrack,
    start: usize,
    publish: &mut impl FnMut(FrameImage, f64) -> AppResult<bool>,
) -> Result<(), PipelineError> {
    stage(shared, job, buffering::VideoWork::Budget);
    let permit = crate::budget::DecoderPermit::acquire(
        if backend == Backend::Software {
            shared.config.decoder_threads
        } else {
            1
        },
        &job.cancel,
    )?;
    stage(shared, job, buffering::VideoWork::Decode);
    let codec = match track.codec {
        TrackCodec::Hevc => Codec::Hevc,
        TrackCodec::H264 => Codec::H264,
        _ => return Err(AppError::Media("invalid FFmpeg video codec".into()).into()),
    };
    let scale = i32::try_from(track.info.media_timescale)
        .map_err(|_| AppError::Media("video timebase exceeds decoder range".into()))?;
    // Job retains the supplied DecodeDevice lease until this codec context is destroyed.
    let mut decoder = unsafe {
        VideoDecoder::new(DecoderConfig {
            codec,
            backend,
            extradata: track.extradata.clone(),
            time_base: (1, scale),
            threads: permit.threads(),
        })
    }?;
    let mut bytes = Vec::new();
    for (index, &(offset, length)) in track.info.samples.iter().enumerate().skip(start) {
        read_sample(shared, job, offset, length, &mut bytes)?;
        if job.cancel.load(Ordering::Acquire) {
            return Err(AppError::Cancelled.into());
        }
        let time = track.samples[index];
        let packet = Packet {
            data: &bytes,
            pts: Some(time.pts),
            dts: Some(time.dts),
            duration: i64::from(time.duration),
        };
        while !decoder.send(&packet)? {
            if !drain_video(&mut decoder, publish)? {
                return Ok(());
            }
        }
        if !drain_video(&mut decoder, publish)? {
            return Ok(());
        }
    }
    while !decoder.drain()? {
        if !drain_video(&mut decoder, publish)? {
            return Ok(());
        }
    }
    drain_video(&mut decoder, publish)?;
    Ok(())
}

fn drain_video(
    decoder: &mut VideoDecoder,
    publish: &mut impl FnMut(FrameImage, f64) -> AppResult<bool>,
) -> Result<bool, PipelineError> {
    while let Some(frame) = decoder.receive()? {
        let pts = frame
            .pts()
            .ok_or_else(|| AppError::Media("decoded video frame has no source PTS".into()))?;
        if !publish(frame.into_image()?, pts)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn decode_vvdec(
    shared: &Shared,
    job: &Job,
    track: &IndexedTrack,
    start: usize,
    publish: &mut impl FnMut(FrameImage, f64) -> AppResult<bool>,
) -> Result<(), PipelineError> {
    stage(shared, job, buffering::VideoWork::Budget);
    let permit = crate::budget::DecoderPermit::acquire(shared.config.decoder_threads, &job.cancel)?;
    stage(shared, job, buffering::VideoWork::Decode);
    let mut decoder = hongguo_bytevc2::VvdecDecoder::new(permit.threads() as i32)?;
    let length_size = hongguo_bytevc2::length_size_from_extradata(&track.extradata)?;
    let parameters = hongguo_bytevc2::annexb_from_extradata(&track.extradata)?;
    let mut bytes = Vec::new();
    for (index, &(offset, length)) in track.info.samples.iter().enumerate().skip(start) {
        read_sample(shared, job, offset, length, &mut bytes)?;
        if job.cancel.load(Ordering::Acquire) {
            return Err(AppError::Cancelled.into());
        }
        let mut packet = if index == start {
            parameters.clone()
        } else {
            Vec::new()
        };
        packet.extend(hongguo_bytevc2::sample_to_annexb_checked(
            &bytes,
            length_size,
        )?);
        let pts = track.samples[index].pts as f64 / f64::from(track.info.media_timescale);
        if let Some(frame) = decoder.decode(&packet, Some(pts), index == start)? {
            let pts = frame
                .pts
                .ok_or_else(|| AppError::Media("ByteVC2 frame has no PTS".into()))?;
            if !publish(FrameImage::Cpu(Arc::new(frame)), pts)? {
                return Ok(());
            }
        }
    }
    while let Some(frame) = decoder.flush()? {
        let pts = frame
            .pts
            .ok_or_else(|| AppError::Media("ByteVC2 frame has no PTS".into()))?;
        if !publish(FrameImage::Cpu(Arc::new(frame)), pts)? {
            break;
        }
    }
    Ok(())
}

fn publish(
    shared: &Shared,
    job: &Job,
    track: &IndexedTrack,
    segment: &hongguo_video::timing::EditSegment,
    image: FrameImage,
    source_pts: f64,
) -> AppResult<bool> {
    stage(shared, job, buffering::VideoWork::Queue);
    let media_start = segment
        .media_start_secs
        .ok_or_else(|| AppError::Media("empty edit cannot publish video".into()))?;
    let source_duration = track.duration_for_pts(source_pts)?;
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
    if duration <= 0.0 || pts + duration < job.target_secs {
        return Ok(true);
    }
    let image = metadata_image(
        image,
        job.device.clone(),
        track.rotation_degrees,
        &track.color,
    );
    let descriptor = image.descriptor();
    let bytes = match &image {
        FrameImage::Cpu(frame) => frame.planes.iter().map(|plane| plane.data.len()).sum(),
        FrameImage::Hardware(_) => descriptor
            .width
            .saturating_mul(descriptor.height)
            .saturating_mul(8),
    };
    if bytes > shared.config.video_queue_bytes {
        return Err(AppError::Media(
            "decoded frame exceeds playback byte budget".into(),
        ));
    }
    let mut state = shared.state.lock();
    while state.epoch == job.epoch
        && state.check().is_ok()
        && !job.cancel.load(Ordering::Acquire)
        && (state.queue.len() >= shared.config.video_queue_frames
            || state.queue_bytes + bytes > shared.config.video_queue_bytes
            || (!state.holding
                && pts - shared.position_locked(&state) > shared.config.video_ahead_secs))
    {
        shared.changed.wait(&mut state);
    }
    shared.check_recovery_deadline(&mut state)?;
    state.check()?;
    if state.epoch != job.epoch || job.cancel.load(Ordering::Acquire) {
        return Err(AppError::Cancelled);
    }
    let serial = state.next_serial;
    state.stream_key = Some(StreamKey {
        width: descriptor.width as u32,
        height: descriptor.height as u32,
        bit_depth: descriptor.bit_depth,
        ..job.stream_key
    });
    state.next_serial = serial
        .checked_add(1)
        .ok_or_else(|| AppError::Media("frame serial exhausted".into()))?;
    state.queue_bytes += bytes;
    state.queue.push_back(QueuedFrame {
        bytes,
        frame: Arc::new(PresentationFrame {
            serial,
            generation: job.epoch,
            pts_secs: pts,
            duration_secs: duration,
            image,
        }),
    });
    shared.changed.notify_all();
    state.video_work = (buffering::VideoWork::Decode, Instant::now());
    Ok(segment.media_rate != 0.0)
}

fn stage(shared: &Shared, job: &Job, stage: buffering::VideoWork) {
    let mut state = shared.state.lock();
    if state.epoch == job.epoch {
        state.video_work = (stage, Instant::now());
    }
}

fn read_sample(
    shared: &Shared,
    job: &Job,
    offset: u64,
    length: u64,
    bytes: &mut Vec<u8>,
) -> AppResult<()> {
    stage(shared, job, buffering::VideoWork::SourceRead);
    shared.source.read_at(offset, length, &job.cancel, bytes)?;
    stage(shared, job, buffering::VideoWork::Decode);
    Ok(())
}

struct MetadataLease {
    inner: Arc<dyn HardwareFrame>,
    _device: Option<Arc<dyn DecodeDevice>>,
    descriptor: FrameDescriptor,
}
unsafe impl HardwareFrame for MetadataLease {
    fn handle(&self) -> HardwareHandle {
        self.inner.handle()
    }
    fn descriptor(&self) -> FrameDescriptor {
        self.descriptor.clone()
    }
    fn present_at(&self, nanos: i64) -> Result<(), String> {
        self.inner.present_at(nanos)
    }
}

fn metadata_image(
    image: FrameImage,
    device: Option<Arc<dyn DecodeDevice>>,
    rotation: u16,
    container: &ColorMetadata,
) -> FrameImage {
    let mut descriptor = image.descriptor();
    descriptor.rotation_degrees = rotation;
    let color = &mut descriptor.color;
    color.primaries = container
        .primaries
        .filter(|value| *value != 2)
        .or(color.primaries);
    color.transfer = container
        .transfer
        .filter(|value| *value != 2)
        .or(color.transfer);
    color.matrix = container
        .matrix
        .filter(|value| *value != 2)
        .or(color.matrix);
    color.full_range = container.full_range.or(color.full_range);
    color.sample_aspect_ratio = container.sample_aspect_ratio.or(color.sample_aspect_ratio);
    match image {
        FrameImage::Cpu(frame) => {
            let mut frame = Arc::unwrap_or_clone(frame);
            frame.color = descriptor.color;
            frame.rotation_degrees = rotation;
            FrameImage::Cpu(Arc::new(frame))
        }
        FrameImage::Hardware(inner) => FrameImage::Hardware(Arc::new(MetadataLease {
            inner,
            _device: device,
            descriptor,
        })),
    }
}

#[cfg(test)]
#[path = "session_video_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "session_video_failures.rs"]
mod failure_tests;
