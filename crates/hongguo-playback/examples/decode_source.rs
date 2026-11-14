use hongguo_codec_ffi::{AudioDecoder, AudioFormat, Codec, Packet, VideoDecoder};
use hongguo_common::{AppError, AppResult};
use hongguo_playback::{IndexedSource, TrackCodec};
use std::{
    env,
    fs::File,
    io::{Read, Seek, SeekFrom},
    time::Instant,
};

const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

fn read(file: &mut File, offset: u64, count: u64, bytes: &mut Vec<u8>) -> AppResult<()> {
    if count > MAX_READ_BYTES {
        return Err(AppError::Media("audit read exceeds byte budget".into()));
    }
    file.seek(SeekFrom::Start(offset))?;
    bytes.resize(count as usize, 0);
    file.read_exact(bytes)?;
    Ok(())
}

#[derive(Default)]
struct Frames {
    count: usize,
    size: (usize, usize),
    bit_depth: u8,
    first_pts: Option<f64>,
    last_pts: Option<f64>,
}

impl Frames {
    fn frame(&mut self, frame: hongguo_media_types::NativeFrame) -> AppResult<()> {
        if self
            .last_pts
            .zip(frame.pts)
            .is_some_and(|(previous, current)| current < previous)
        {
            return Err(AppError::Media(
                "decoder emitted decreasing source PTS".into(),
            ));
        }
        self.count += 1;
        self.size = (frame.width, frame.height);
        self.bit_depth = frame.bit_depth;
        if self.first_pts.is_none() {
            self.first_pts = frame.pts;
        }
        self.last_pts = frame.pts;
        Ok(())
    }
}

fn video(file: &mut File, index: &IndexedSource) -> Result<Frames, Box<dyn std::error::Error>> {
    let mut frames = Frames::default();
    let track = &index.video;
    if track.codec == TrackCodec::Bytevc2 {
        let (_, moov) =
            hongguo_bytevc2::read_moov_at(file.metadata()?.len(), |offset, size, bytes| {
                read(file, offset, size, bytes)
            })?;
        let track = hongguo_bytevc2::track_from_moov(&moov)?;
        hongguo_bytevc2::decode_track_vvdec_native(
            &track,
            0,
            |offset, size, bytes| read(file, offset, size, bytes),
            &mut |frame| frames.frame(frame),
        )?;
        return Ok(frames);
    }
    let codec = match track.codec {
        TrackCodec::Hevc => Codec::Hevc,
        TrackCodec::H264 => Codec::H264,
        _ => return Err("unsupported audit video codec".into()),
    };
    let mut decoder = VideoDecoder::software(
        codec,
        track.extradata.clone(),
        (1, track.info.media_timescale as i32),
    )?;
    let mut receive = |decoder: &mut VideoDecoder| -> Result<(), Box<dyn std::error::Error>> {
        while let Some(frame) = decoder.receive()? {
            frames.frame(frame.cpu()?)?;
        }
        Ok(())
    };
    let mut bytes = Vec::new();
    for ((offset, size), time) in track.info.samples.iter().zip(&track.samples) {
        read(file, *offset, *size, &mut bytes)?;
        let packet = Packet {
            data: &bytes,
            pts: Some(time.pts),
            dts: Some(time.dts),
            duration: i64::from(time.duration),
        };
        while !decoder.send(&packet)? {
            receive(&mut decoder)?;
        }
        receive(&mut decoder)?;
    }
    while !decoder.drain()? {
        receive(&mut decoder)?;
    }
    receive(&mut decoder)?;
    Ok(frames)
}

fn audio(
    file: &mut File,
    index: &IndexedSource,
    rate: f64,
) -> Result<(usize, f64, f32), Box<dyn std::error::Error>> {
    let Some(track) = &index.audio else {
        return Ok((0, 0.0, 0.0));
    };
    let format = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
    };
    let mut decoder =
        AudioDecoder::new(&track.extradata, track.info.media_timescale, format, rate)?;
    let mut frames = 0;
    let mut media_duration = 0.0;
    let mut peak = 0_f32;
    let mut receive = |decoder: &mut AudioDecoder| -> Result<(), Box<dyn std::error::Error>> {
        while let Some(block) = decoder.receive()? {
            if block.samples.iter().any(|sample| !sample.is_finite()) {
                return Err("nonfinite decoded PCM".into());
            }
            frames += block.samples.len() / usize::from(format.channels);
            media_duration += block.media_duration_secs;
            peak = block
                .samples
                .iter()
                .fold(peak, |peak, sample| peak.max(sample.abs()));
        }
        Ok(())
    };
    let mut bytes = Vec::new();
    for ((offset, size), time) in track.info.samples.iter().zip(&track.samples) {
        read(file, *offset, *size, &mut bytes)?;
        while !decoder.send(&bytes, Some(time.pts))? {
            receive(&mut decoder)?;
        }
        receive(&mut decoder)?;
    }
    while !decoder.drain()? {
        receive(&mut decoder)?;
    }
    receive(&mut decoder)?;
    Ok((frames, media_duration, peak))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let path = arguments
        .next()
        .ok_or("Usage: decode_source <plaintext MP4> [rate]")?;
    let rate = arguments
        .next()
        .map(|rate| rate.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    let mut file = File::open(path)?;
    let index = IndexedSource::open(file.metadata()?.len(), |offset, size, bytes| {
        read(&mut file, offset, size, bytes)
    })?;
    let started = Instant::now();
    let frames = video(&mut file, &index)?;
    let video_elapsed = started.elapsed();
    let (pcm_frames, audio_secs, pcm_peak) = audio(&mut file, &index, rate)?;
    println!(
        "codec={:?} samples={} decoded={} size={:?} bitDepth={} firstPTS={:?} lastPTS={:?} videoSeconds={:.3} decodeFPS={:.1} rate={rate} pcmFrames={pcm_frames} audioMediaSeconds={audio_secs:.3} pcmPeak={pcm_peak:.6}",
        index.video.codec,
        index.video.info.samples.len(),
        frames.count,
        frames.size,
        frames.bit_depth,
        frames.first_pts,
        frames.last_pts,
        video_elapsed.as_secs_f64(),
        frames.count as f64 / video_elapsed.as_secs_f64()
    );
    if frames.count != index.video.info.samples.len() {
        return Err("decoded frame count differs from source access units".into());
    }
    Ok(())
}
