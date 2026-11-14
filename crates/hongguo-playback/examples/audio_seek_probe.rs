//! Offline PCM comparison of cold AAC seeks against a continuous decode.
//! Usage: audio_seek_probe <plaintext MP4> <target seconds> [playback rate] [PCM rate]
use hongguo_codec_ffi::{AudioDecoder, AudioFormat, PcmBlock};
use hongguo_common::{AppError, AppResult};
use hongguo_playback::{IndexedSource, IndexedTrack};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

fn read(file: &mut File, offset: u64, size: u64, bytes: &mut Vec<u8>) -> AppResult<()> {
    if size > 64 * 1024 * 1024 {
        return Err(AppError::Media("sample exceeds audit read budget".into()));
    }
    file.seek(SeekFrom::Start(offset))?;
    bytes.resize(size as usize, 0);
    file.read_exact(bytes)?;
    Ok(())
}

fn decode(
    file: &mut File,
    track: &IndexedTrack,
    start: usize,
    target: f64,
    rate: f64,
    sample_rate: u32,
) -> Result<(Vec<f32>, usize), Box<dyn std::error::Error>> {
    let output = AudioFormat {
        sample_rate,
        channels: 2,
    };
    let mut decoder =
        AudioDecoder::new(&track.extradata, track.info.media_timescale, output, rate)?;
    if start > 0 {
        let mut bytes = Vec::new();
        for index in 0..2.min(track.samples.len()) {
            let (offset, size) = track.info.samples[index];
            read(file, offset, size, &mut bytes)?;
            while !decoder.send(&bytes, Some(track.samples[index].pts))? {
                while decoder.receive()?.is_some() {}
            }
            while decoder.receive()?.is_some() {}
        }
        decoder.reset(rate)?;
    }
    let mut result = vec![f32::NAN; sample_rate as usize * 2];
    let mut consumed = 0;
    let mut capture = |block: PcmBlock| {
        let Some(pts) = block.media_start_secs else {
            return;
        };
        let frames = block.samples.len() / 2;
        for frame in 0..frames {
            let media = pts + frame as f64 * block.media_duration_secs / frames as f64;
            let destination = ((media - target) * f64::from(sample_rate) / rate).round() as i64;
            if destination >= 0 && (destination as usize) < result.len() / 2 {
                result[destination as usize * 2..destination as usize * 2 + 2]
                    .copy_from_slice(&block.samples[frame * 2..frame * 2 + 2]);
            }
        }
    };
    let mut receive = |decoder: &mut AudioDecoder| -> Result<(), Box<dyn std::error::Error>> {
        while let Some(block) = decoder.receive()? {
            capture(block);
        }
        Ok(())
    };
    let mut bytes = Vec::new();
    for index in start..track.samples.len() {
        let time = &track.samples[index];
        if time.pts as f64 / f64::from(track.info.media_timescale) > target + 2.0 * rate + 0.5 {
            break;
        }
        let (offset, size) = track.info.samples[index];
        read(file, offset, size, &mut bytes)?;
        consumed += 1;
        while !decoder.send(&bytes, Some(time.pts))? {
            receive(&mut decoder)?;
        }
        receive(&mut decoder)?;
    }
    while !decoder.drain()? {
        receive(&mut decoder)?;
    }
    receive(&mut decoder)?;
    Ok((result, consumed))
}

struct Statistics {
    energies: [f64; 4],
    coherence: f64,
    bands: [f64; 8],
}
fn statistics(samples: &[f32], sample_rate: u32) -> Statistics {
    let mut energies = [0.; 4];
    let mut cross = 0.;
    for pair in samples
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| pair.iter().all(|value| value.is_finite()))
    {
        let (left, right) = (f64::from(pair[0]), f64::from(pair[1]));
        for (energy, value) in energies
            .iter_mut()
            .zip([left, right, left + right, left - right])
        {
            *energy += value * value;
        }
        cross += left * right;
    }
    let coherence = cross / (energies[0] * energies[1]).sqrt();
    let mut bands = [0.; 8];
    let edges = [
        0.,
        125.,
        250.,
        500.,
        1000.,
        2000.,
        4000.,
        8000.,
        f64::from(sample_rate) / 2.,
    ];
    let window: Vec<_> = (0..2048)
        .map(|index| 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / 2047.).cos())
        .collect();
    for (band, energy) in bands.iter_mut().enumerate() {
        for frequency in 0..4 {
            let hz = edges[band] + (edges[band + 1] - edges[band]) * (frequency as f64 + 0.5) / 4.;
            let coefficient = 2. * (std::f64::consts::TAU * hz / f64::from(sample_rate)).cos();
            for block in samples.as_chunks::<4096>().0 {
                for channel in 0..2 {
                    let (mut previous, mut before) = (0., 0.);
                    for (index, weight) in window.iter().enumerate() {
                        let sample = block[index * 2 + channel];
                        let value = if sample.is_finite() {
                            f64::from(sample) * weight
                        } else {
                            0.
                        };
                        let next = value + coefficient * previous - before;
                        before = previous;
                        previous = next;
                    }
                    *energy += (previous * previous + before * before
                        - coefficient * previous * before)
                        .max(0.);
                }
            }
        }
    }
    Statistics {
        energies,
        coherence,
        bands,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut file = File::open(args.next().ok_or("expected plaintext MP4 path")?)?;
    let target = args
        .next()
        .ok_or("expected target seconds")?
        .parse::<f64>()?;
    let rate = args
        .next()
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    let sample_rate = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(48_000);
    if !(8000..=192000).contains(&sample_rate) || !target.is_finite() || target < 0.0 {
        return Err("invalid probe rate or target".into());
    }
    let index = IndexedSource::open(file.metadata()?.len(), |offset, size, bytes| {
        read(&mut file, offset, size, bytes)
    })?;
    let track = index.audio.as_ref().ok_or("no AAC track")?;
    println!(
        "ASC={:02x?} timescale={} samples={} target={target} rate={rate}",
        track.extradata,
        track.info.media_timescale,
        track.samples.len()
    );
    let (baseline, total) = decode(&mut file, track, 0, target, rate, sample_rate)?;
    let baseline_statistics = statistics(&baseline, sample_rate);
    for preroll in [0.0, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0, f64::NAN] {
        let start = if preroll.is_nan() {
            track.audio_seek_sample(target, rate, sample_rate)
        } else {
            track
                .samples
                .partition_point(|sample| {
                    sample.pts as f64 / f64::from(track.info.media_timescale) < target - preroll
                })
                .saturating_sub(1)
        };
        let (samples, consumed) = decode(&mut file, track, start, target, rate, sample_rate)?;
        let stats = statistics(&samples, sample_rate);
        let db = |a: f64, b: f64| 10. * (a / b).log10();
        let maximum_band = baseline_statistics.bands.iter().copied().fold(0., f64::max);
        let band_error = baseline_statistics
            .bands
            .iter()
            .zip(stats.bands)
            .filter(|(base, _)| **base > maximum_band * 1e-5)
            .map(|(base, current)| db(current, *base).abs())
            .fold(0., f64::max);
        let mut maximum = 0_f64;
        let mut error = 0_f64;
        let mut energy = 0_f64;
        let mut missing = 0;
        for (a, b) in baseline.iter().zip(&samples) {
            if !a.is_finite() || !b.is_finite() {
                missing += 1;
                continue;
            }
            let delta = f64::from(a - b);
            maximum = maximum.max(delta.abs());
            error += delta * delta;
            energy += f64::from(*a).powi(2);
        }
        let rms = (error / (samples.len() - missing).max(1) as f64).sqrt();
        let snr = 10.0 * (energy / error).log10();
        let mut best = (f64::INFINITY, 0);
        for lag in -64..=64 {
            let mut square = 0.0;
            let mut count = 0;
            for frame in (64..sample_rate as usize - 64).step_by(16) {
                let a = baseline[frame * 2] + baseline[frame * 2 + 1];
                let b = samples[(frame as i32 + lag) as usize * 2]
                    + samples[(frame as i32 + lag) as usize * 2 + 1];
                if a.is_finite() && b.is_finite() {
                    square += f64::from(a - b).powi(2);
                    count += 1;
                }
            }
            let error = (square / count.max(1) as f64).sqrt();
            if error < best.0 {
                best = (error, lag);
            }
        }
        let mut aligned = 0.0;
        let mut side = 0.0;
        let mut aligned_energy = 0.0;
        let mut count = 0;
        for frame in 64..sample_rate as usize - 64 {
            let index = (frame as i32 + best.1) as usize * 2;
            let (a, b) = (baseline[frame * 2], baseline[frame * 2 + 1]);
            let (c, d) = (samples[index], samples[index + 1]);
            if [a, b, c, d].iter().all(|value| value.is_finite()) {
                aligned += f64::from(a - c).powi(2) + f64::from(b - d).powi(2);
                side += f64::from((a - b) - (c - d)).powi(2);
                aligned_energy += f64::from(a).powi(2) + f64::from(b).powi(2);
                count += 1;
            }
        }
        println!(
            "preroll={preroll:.2}s startSample={start} packets={consumed}/{total} missing={missing} maxError={maximum:.9} rmsError={rms:.9} snrDb={snr:.3} midRms={:.9} bestLag={} alignedSnrDb={:.3} sideRms={:.9}",
            best.0,
            best.1,
            10.0 * (aligned_energy / aligned).log10(),
            (side / count.max(1) as f64).sqrt()
        );
        println!(
            "energyDb L={:.4} R={:.4} mid={:.4} side={:.4} coherence={:.5}/{:.5} maxBandDb={band_error:.4}",
            db(stats.energies[0], baseline_statistics.energies[0]),
            db(stats.energies[1], baseline_statistics.energies[1]),
            db(stats.energies[2], baseline_statistics.energies[2]),
            db(stats.energies[3], baseline_statistics.energies[3]),
            stats.coherence,
            baseline_statistics.coherence
        );
    }
    Ok(())
}
