use std::io::{Read, Seek, SeekFrom, Write};

use hongguo_common::{AppError, AppResult};

use super::{ArchiveHevc, TimedSample};
use crate::mp4::{build_moov, mdat_data_start, mdat_header, needs_wide_offsets};
use crate::mux::track::{AudioSpec, VideoSpec};
use crate::mux::{movie_duration, runs_i32, runs_u32};

pub(super) struct RecordedSample {
    offset: u64,
    size: u32,
    duration: u32,
    pts: i64,
    sync: bool,
}

pub(super) fn record(
    out: &mut Vec<RecordedSample>,
    frames: &[TimedSample],
    offset: &mut u64,
) -> AppResult<()> {
    for frame in frames {
        let size = u32::try_from(frame.data.len())
            .map_err(|_| AppError::Media("归档样本超过 MP4 上限".into()))?;
        out.push(RecordedSample {
            offset: *offset,
            size,
            duration: frame.dur,
            pts: frame.dts as i64 + i64::from(frame.cto),
            sync: frame.sync,
        });
        *offset += u64::from(size);
    }
    Ok(())
}

struct SampleTimes {
    stts: Vec<(u32, u32)>,
    ctts: Vec<(u32, i32)>,
}

fn times(samples: &[RecordedSample]) -> AppResult<SampleTimes> {
    let stts = runs_u32(
        &samples
            .iter()
            .map(|sample| sample.duration)
            .collect::<Vec<_>>(),
    );
    let mut dts = 0_i64;
    let mut offsets = Vec::with_capacity(samples.len());
    for sample in samples {
        offsets.push(
            i32::try_from(sample.pts - dts)
                .map_err(|_| AppError::Media("归档合成偏移超过 MP4 上限".into()))?,
        );
        dts += i64::from(sample.duration);
    }
    let ctts = if offsets.iter().any(|offset| *offset != 0) {
        runs_i32(&offsets)
    } else {
        Vec::new()
    };
    Ok(SampleTimes { stts, ctts })
}

impl ArchiveHevc {
    pub fn finish(mut self) -> AppResult<u64> {
        let config = self
            .config
            .take()
            .ok_or_else(|| AppError::Media("归档没有视频参数集".into()))?;
        let SampleTimes { stts, ctts } = times(&self.video_samples)?;
        let video = VideoSpec {
            track_id: 1,
            width: self.width as u32,
            height: self.height as u32,
            hevc: config,
            sizes: self
                .video_samples
                .iter()
                .map(|sample| sample.size)
                .collect(),
            duration: self
                .video_samples
                .iter()
                .map(|sample| u64::from(sample.duration))
                .sum(),
            stts,
            ctts,
            stss: Some(
                self.video_samples
                    .iter()
                    .enumerate()
                    .filter_map(|(index, sample)| sample.sync.then_some(index as u32 + 1))
                    .collect(),
            ),
        };
        let mut tracks = vec![video.build()];
        if let Some(audio) = &self.audio {
            let SampleTimes { stts, ctts } = times(&self.audio_samples)?;
            let mut track = AudioSpec {
                track_id: 2,
                channels: audio.channels,
                sample_rate: audio.sample_rate,
                esds: audio.esds.clone(),
                sizes: self
                    .audio_samples
                    .iter()
                    .map(|sample| sample.size)
                    .collect(),
                duration: self
                    .audio_samples
                    .iter()
                    .map(|sample| u64::from(sample.duration))
                    .sum(),
                stts,
            }
            .build();
            track.ctts = ctts;
            tracks.push(track);
        }
        let mvhd = crate::mux::track::mvhd(movie_duration(&tracks), tracks.len() as u32 + 1);
        let total_payload = tracks
            .iter()
            .flat_map(|track| &track.sizes)
            .map(|size| u64::from(*size))
            .sum();
        let wide = needs_wide_offsets(total_payload);
        let ftyp = crate::mux::ftyp();
        let mut offsets: Vec<Vec<u64>> = tracks
            .iter()
            .map(|track| vec![0; track.sizes.len()])
            .collect();
        let moov_len = build_moov(&mvhd, &tracks, &offsets, wide).len();
        let mut cursor = mdat_data_start(ftyp.len(), moov_len, wide);
        let mut order: Vec<_> = [&self.video_samples, &self.audio_samples]
            .into_iter()
            .enumerate()
            .flat_map(|(track, samples)| {
                samples
                    .iter()
                    .enumerate()
                    .map(move |(index, sample)| (track, index, sample))
            })
            .collect();
        order.sort_by_key(|(_, _, sample)| sample.offset);
        for &(track, index, sample) in &order {
            offsets[track][index] = cursor;
            cursor += u64::from(sample.size);
        }
        let moov = build_moov(&mvhd, &tracks, &offsets, wide);
        let temp = hongguo_common::utils::path::temp_path_for(&self.path);
        hongguo_common::offline_qa::qa_clear_for_replace(&temp).map_err(io)?;
        let source = hongguo_common::offline_qa::prepare_qa_io(&self.path).map_err(AppError::Io)?;
        let mut input = std::fs::File::open(source).map_err(io)?;
        let result = (|| {
            let mut output = hongguo_common::offline_qa::qa_create(&temp).map_err(io)?;
            output.write_all(&ftyp).map_err(io)?;
            output.write_all(&moov).map_err(io)?;
            output
                .write_all(&mdat_header(total_payload, wide))
                .map_err(io)?;
            for &(_, _, sample) in &order {
                input.seek(SeekFrom::Start(sample.offset)).map_err(io)?;
                let copied =
                    std::io::copy(&mut (&mut input).take(u64::from(sample.size)), &mut output)
                        .map_err(io)?;
                if copied != u64::from(sample.size) {
                    return Err(AppError::Media("归档样本字节截断".into()));
                }
            }
            output.flush().map_err(io)?;
            output.metadata().map(|metadata| metadata.len()).map_err(io)
        })();
        drop(self.file);
        drop(input);
        match result {
            Ok(length) => {
                hongguo_common::offline_qa::qa_rename(&temp, &self.path).map_err(|error| {
                    let _ = hongguo_common::offline_qa::qa_remove_file(&temp);
                    io(error)
                })?;
                Ok(length)
            }
            Err(error) => {
                let _ = hongguo_common::offline_qa::qa_remove_file(&temp);
                Err(error)
            }
        }
    }
}

fn io(error: std::io::Error) -> AppError {
    AppError::Io(error.to_string())
}
