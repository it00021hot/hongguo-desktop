//! 可追加的 HEVC fMP4。首个封闭 GOP 写成独立片段后文件即可从 0 解起，后续 GOP 只追加 `moof+mdat`。
//! 电影/轨道声明整段时间轴；初始媒体表为空，其 `mdhd` 时长必须为零，实际时间由片段给出。

use std::fs::File;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

#[path = "archive.rs"]
mod archive;

#[cfg(test)]
#[path = "fragment_inspection.rs"]
mod inspection;
#[cfg(test)]
use inspection::{prefix_is_independently_decodable, read_fragment_video, read_u32};

use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::{build_box, parse_boxes};

use super::hvcc;
use super::track::{audio_stsd, dinf, hdlr, mdhd, mvhd, smhd, tkhd, video_stsd, vmhd};
use super::{AudioTrack, MEDIA_TIMESCALE, MOVIE_TIMESCALE};

#[cfg(test)]
#[path = "fragment_timing_tests.rs"]
mod timing_tests;

const VIDEO_TRACK: u32 = 1;
const AUDIO_TRACK: u32 = 2;
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x020000;
const TRUN_FLAGS: u32 = 0x0001 | 0x0100 | 0x0200 | 0x0400 | 0x0800;
const SAMPLE_SYNC: u32 = 0x0200_0000;
const SAMPLE_NON_SYNC: u32 = 0x0101_0000;

struct AudioHeader {
    channels: u16,
    sample_rate: u32,
    esds: Option<Vec<u8>>,
}

/// 分批写入归档样本；必须调用 finish 封存为普通 faststart MP4。
pub struct ArchiveHevc {
    file: File,
    path: PathBuf,
    sequence: u32,
    timeline_secs: f64,
    width: usize,
    height: usize,
    audio: Option<AudioHeader>,
    started: bool,
    config: Option<hvcc::HevcConfig>,
    video_samples: Vec<archive::RecordedSample>,
    audio_samples: Vec<archive::RecordedSample>,
}

/// 一次追加后文件上能看到的视频时间。
#[cfg(test)]
struct FragmentVideo {
    pub pts: Vec<f64>,
    pub first_is_sync: bool,
    pub fragment_bases: Vec<u64>,
}

pub struct ArchiveTiming {
    pub video_end_secs: f64,
    pub audio_end_secs: Option<f64>,
}

impl ArchiveHevc {
    pub fn create(
        path: &Path,
        width: usize,
        height: usize,
        audio: Option<&AudioTrack>,
        timeline_secs: f64,
    ) -> AppResult<Self> {
        if width == 0 || height == 0 || timeline_secs <= 0.0 {
            return Err(AppError::Media("片段封装缺少分辨率或时长".into()));
        }
        if let Some(parent) = path.parent() {
            hongguo_common::offline_qa::qa_create_dir_all(parent)
                .map_err(|e| AppError::Io(e.to_string()))?;
        }
        let file =
            hongguo_common::offline_qa::qa_create(path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            sequence: 1,
            timeline_secs,
            width,
            height,
            audio: audio.map(|track| AudioHeader {
                channels: track.channels,
                sample_rate: track.sample_rate,
                esds: track.esds.clone(),
            }),
            started: false,
            config: None,
            video_samples: Vec::new(),
            audio_samples: Vec::new(),
        })
    }

    /// 写入一个 GOP 以及落在它时间范围内的音频。`video` 的第一帧必须是关键帧。
    pub fn push(
        &mut self,
        video: &[(f64, Vec<u8>, bool)],
        audio: &[(f64, Vec<u8>)],
        timing: ArchiveTiming,
    ) -> AppResult<u64> {
        if video.is_empty()
            || (!self.started
                && (!video[0].2
                    || !hvcc::split(&video[0].1)
                        .into_iter()
                        .any(crate::hevc::is_random_access_nal)))
        {
            return Err(AppError::Media("片段缺少同步样本".into()));
        }
        let timed = time_samples(video, true, timing.video_end_secs)?;
        let audio_timed = time_samples(
            &audio
                .iter()
                .map(|(pts, data)| (*pts, data.clone(), true))
                .collect::<Vec<_>>(),
            false,
            if audio.is_empty() {
                0.0
            } else {
                timing
                    .audio_end_secs
                    .ok_or_else(|| AppError::Media("音频片段缺少源时长边界".into()))?
            },
        )?;
        if !self.started {
            self.config = Some(hvcc::extract(&video[0].1)?);
            let init = init_segment(
                self.width,
                self.height,
                self.audio.as_ref(),
                &video[0].1,
                self.timeline_secs,
            )?;
            self.file
                .write_all(&init)
                .map_err(|e| AppError::Io(e.to_string()))?;
            self.started = true;
        }
        let fragment = fragment_bytes(self.sequence, &timed, &audio_timed)?;
        let position = self
            .file
            .stream_position()
            .map_err(|error| AppError::Io(error.to_string()))?;
        let mut offset = position
            + u64::from(u32::from_be_bytes(
                fragment[..4].try_into().expect("moof length"),
            ))
            + 8;
        archive::record(&mut self.video_samples, &timed, &mut offset)?;
        archive::record(&mut self.audio_samples, &audio_timed, &mut offset)?;
        self.sequence += 1;
        self.file
            .write_all(&fragment)
            .map_err(|e| AppError::Io(e.to_string()))?;
        self.file.flush().map_err(|e| AppError::Io(e.to_string()))?;
        let bytes = self
            .file
            .metadata()
            .map_err(|e| AppError::Io(e.to_string()))?
            .len();
        Ok(bytes)
    }
}

/// 编码器输出的一帧：显示时间、Annex-B、是否同步样本。
pub type AnnexbFrame = (f64, Vec<u8>, bool);

/// 取最多 `count` 帧，在下一同步样本前截断以保留片段边界。
pub fn take_run(
    units: &mut Vec<AnnexbFrame>,
    count: usize,
    require_sync: bool,
) -> Option<(Vec<AnnexbFrame>, Option<f64>)> {
    if count == 0 || units.is_empty() {
        return None;
    }
    if require_sync && !units[0].2 {
        return None;
    }
    let count = units
        .iter()
        .enumerate()
        .skip(1)
        .take(count - 1)
        .find_map(|(index, unit)| unit.2.then_some(index))
        .unwrap_or(count);
    if units.len() <= count {
        return None;
    }
    let next = units.get(count).map(|unit| unit.0);
    Some((units.drain(0..count).collect(), next))
}

/// 第二个关键帧出现时，把它之前的样本切成一个封闭 GOP，并返回后一个关键帧的显示时间。
pub fn split_completed_gop(units: &mut Vec<AnnexbFrame>) -> Option<(Vec<AnnexbFrame>, f64)> {
    let mut keys = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.2)
        .map(|(index, _)| index);
    let first = keys.next()?;
    if first != 0 {
        return None;
    }
    let second = keys.next()?;
    let next_pts = units[second].0;
    Some((units.drain(0..second).collect(), next_pts))
}

struct TimedSample {
    dts: u64,
    dur: u32,
    cto: i32,
    data: Vec<u8>,
    sync: bool,
}

fn trex_box(track_id: u32) -> Vec<u8> {
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend(track_id.to_be_bytes());
    payload.extend(1u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    build_box(b"trex", &payload)
}

fn time_samples(
    frames: &[(f64, Vec<u8>, bool)],
    as_video: bool,
    end_secs: f64,
) -> AppResult<Vec<TimedSample>> {
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    let pts = frames
        .iter()
        .map(|(secs, _, _)| ticks(*secs))
        .collect::<AppResult<Vec<_>>>()?;
    let dts = decode_times(&pts);
    let durations = durations_of(&dts, ticks(end_secs)?)?;
    frames
        .iter()
        .enumerate()
        .map(|(index, (_, data, sync))| {
            let cto = i32::try_from(pts[index] as i64 - dts[index] as i64)
                .map_err(|_| AppError::Media("合成偏移超出范围".into()))?;
            Ok(TimedSample {
                dts: dts[index],
                dur: durations[index],
                cto,
                data: if as_video {
                    hvcc::to_sample(data)?
                } else {
                    data.clone()
                },
                sync: *sync,
            })
        })
        .collect()
}

fn ticks(secs: f64) -> AppResult<u64> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(AppError::Media(format!("时间戳不能进入片段: {secs}")));
    }
    Ok((secs * f64::from(MEDIA_TIMESCALE)).round() as u64)
}

fn decode_times(pts: &[u64]) -> Vec<u64> {
    if pts.windows(2).all(|pair| pair[1] >= pair[0]) {
        return pts.to_vec();
    }
    let mut sorted = pts.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::with_capacity(sorted.len());
    let mut prev = 0u64;
    for (index, value) in sorted.into_iter().enumerate() {
        let value = if index > 0 && value <= prev {
            prev + 1
        } else {
            value
        };
        out.push(value);
        prev = value;
    }
    out
}

fn durations_of(dts: &[u64], end: u64) -> AppResult<Vec<u32>> {
    dts.iter()
        .copied()
        .zip(dts.iter().copied().skip(1).chain(std::iter::once(end)))
        .map(|(start, end)| {
            end.checked_sub(start)
                .filter(|duration| *duration > 0)
                .and_then(|duration| u32::try_from(duration).ok())
                .ok_or_else(|| AppError::Media("片段样本的真实时长边界无效".into()))
        })
        .collect()
}

fn init_segment(
    width: usize,
    height: usize,
    audio: Option<&AudioHeader>,
    first_video: &[u8],
    timeline_secs: f64,
) -> AppResult<Vec<u8>> {
    let config = hvcc::extract(first_video)?;
    let movie_ticks = (timeline_secs * f64::from(MOVIE_TIMESCALE)).round() as u64;
    let mut moov = mvhd(movie_ticks, if audio.is_some() { 3 } else { 2 });
    moov.extend(video_trak(
        width as u32,
        height as u32,
        movie_ticks,
        &config,
    ));
    if let Some(audio) = audio {
        moov.extend(audio_trak(audio, movie_ticks));
    }
    let mut trex = trex_box(VIDEO_TRACK);
    if audio.is_some() {
        trex.extend(trex_box(AUDIO_TRACK));
    }
    moov.extend(build_box(b"mvex", &trex));
    let mut out = ftyp();
    out.extend(build_box(b"moov", &moov));
    Ok(out)
}

fn ftyp() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"iso5");
    payload.extend(0u32.to_be_bytes());
    payload.extend_from_slice(b"iso5");
    payload.extend_from_slice(b"isom");
    payload.extend_from_slice(b"iso6");
    payload.extend_from_slice(b"mp41");
    build_box(b"ftyp", &payload)
}

fn video_trak(width: u32, height: u32, movie_ticks: u64, config: &hvcc::HevcConfig) -> Vec<u8> {
    trak(TrakParts {
        track_id: VIDEO_TRACK,
        movie_ticks,
        width,
        height,
        volume: 0,
        handler: hdlr(b"vide", "VideoHandler"),
        header: vmhd(),
        stsd: video_stsd(width, height, config),
    })
}

fn audio_trak(audio: &AudioHeader, movie_ticks: u64) -> Vec<u8> {
    trak(TrakParts {
        track_id: AUDIO_TRACK,
        movie_ticks,
        width: 0,
        height: 0,
        volume: 0x0100,
        handler: hdlr(b"soun", "SoundHandler"),
        header: smhd(),
        stsd: audio_stsd(audio.channels, audio.sample_rate, audio.esds.as_deref()),
    })
}

struct TrakParts {
    track_id: u32,
    movie_ticks: u64,
    width: u32,
    height: u32,
    volume: u16,
    handler: Vec<u8>,
    header: Vec<u8>,
    stsd: Vec<u8>,
}

fn trak(parts: TrakParts) -> Vec<u8> {
    // 初始 stts/stsz 没有样本；AVFoundation 会把这里的时长再加到后续 fragments 上。
    let mut mdia = mdhd(MEDIA_TIMESCALE, 0);
    mdia.extend(parts.handler);
    let mut minf = parts.header;
    minf.extend(dinf());
    minf.extend(empty_stbl(parts.stsd));
    mdia.extend(build_box(b"minf", &minf));
    let mut payload = tkhd(
        parts.track_id,
        parts.movie_ticks,
        parts.width,
        parts.height,
        parts.volume,
    );
    payload.extend(build_box(b"mdia", &mdia));
    build_box(b"trak", &payload)
}

fn empty_stbl(stsd: Vec<u8>) -> Vec<u8> {
    let version = 0u32.to_be_bytes();
    let mut stts = version.to_vec();
    stts.extend(0u32.to_be_bytes());
    let mut stsc = version.to_vec();
    stsc.extend(0u32.to_be_bytes());
    let mut stsz = version.to_vec();
    stsz.extend(0u32.to_be_bytes());
    stsz.extend(0u32.to_be_bytes());
    let mut stco = version.to_vec();
    stco.extend(0u32.to_be_bytes());
    let mut payload = stsd;
    payload.extend(build_box(b"stts", &stts));
    payload.extend(build_box(b"stsc", &stsc));
    payload.extend(build_box(b"stsz", &stsz));
    payload.extend(build_box(b"stco", &stco));
    build_box(b"stbl", &payload)
}

fn fragment_bytes(
    sequence: u32,
    video: &[TimedSample],
    audio: &[TimedSample],
) -> AppResult<Vec<u8>> {
    let video_payload: usize = video.iter().map(|s| s.data.len()).sum();
    let audio_payload: usize = audio.iter().map(|s| s.data.len()).sum();
    let video_trun = trun_box(video, 0);
    let audio_trun = (!audio.is_empty()).then(|| trun_box(audio, 0));
    let video_traf = traf_box(VIDEO_TRACK, video[0].dts, &video_trun);
    let audio_traf = audio_trun
        .as_ref()
        .map(|trun| traf_box(AUDIO_TRACK, audio[0].dts, trun));
    let mut payload = mfhd(sequence);
    let video_patch = payload.len() + data_offset_index(&video_traf);
    payload.extend(video_traf);
    let audio_patch = audio_traf
        .as_ref()
        .map(|traf| payload.len() + data_offset_index(traf));
    if let Some(traf) = audio_traf {
        payload.extend(traf);
    }
    let mut moof = build_box(b"moof", &payload);
    let mdat_payload = video_payload + audio_payload;
    let video_offset = moof.len() as u32 + 8;
    patch_u32(&mut moof, 8 + video_patch, video_offset);
    if let Some(at) = audio_patch {
        patch_u32(&mut moof, 8 + at, video_offset + video_payload as u32);
    }
    let mut out = moof;
    let mdat_size = (mdat_payload as u32)
        .checked_add(8)
        .ok_or_else(|| AppError::Media("片段 mdat 超过 4GB".into()))?;
    out.extend(mdat_size.to_be_bytes());
    out.extend_from_slice(b"mdat");
    for sample in video {
        out.extend_from_slice(&sample.data);
    }
    for sample in audio {
        out.extend_from_slice(&sample.data);
    }
    Ok(out)
}

fn data_offset_index(traf: &[u8]) -> usize {
    let children = parse_boxes(traf, 8, traf.len());
    let trun = children
        .iter()
        .find(|b| b.is("trun"))
        .expect("traf 里有 trun");
    trun.start + 8
}

fn patch_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn mfhd(sequence: u32) -> Vec<u8> {
    let mut payload = 0u32.to_be_bytes().to_vec();
    payload.extend(sequence.to_be_bytes());
    build_box(b"mfhd", &payload)
}

fn traf_box(track_id: u32, base: u64, trun: &[u8]) -> Vec<u8> {
    let mut payload = tfhd(track_id);
    payload.extend(tfdt(base));
    payload.extend_from_slice(trun);
    build_box(b"traf", &payload)
}

fn tfhd(track_id: u32) -> Vec<u8> {
    let mut payload = TFHD_DEFAULT_BASE_IS_MOOF.to_be_bytes().to_vec();
    payload.extend(track_id.to_be_bytes());
    build_box(b"tfhd", &payload)
}

fn tfdt(base: u64) -> Vec<u8> {
    let mut payload = Vec::new();
    if base > u64::from(u32::MAX) {
        payload.push(1);
        payload.extend([0u8; 3]);
        payload.extend(base.to_be_bytes());
    } else {
        payload.extend(0u32.to_be_bytes());
        payload.extend((base as u32).to_be_bytes());
    }
    build_box(b"tfdt", &payload)
}

fn trun_box(samples: &[TimedSample], data_offset: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(1);
    payload.extend_from_slice(&TRUN_FLAGS.to_be_bytes()[1..4]);
    payload.extend((samples.len() as u32).to_be_bytes());
    payload.extend(data_offset.to_be_bytes());
    for sample in samples {
        payload.extend(sample.dur.to_be_bytes());
        payload.extend((sample.data.len() as u32).to_be_bytes());
        let flags = if sample.sync {
            SAMPLE_SYNC
        } else {
            SAMPLE_NON_SYNC
        };
        payload.extend(flags.to_be_bytes());
        payload.extend(sample.cto.to_be_bytes());
    }
    build_box(b"trun", &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f64 = 1.0 / 30.0;

    fn annexb(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(nal);
        }
        out
    }

    pub(super) fn idr(at: f64) -> (f64, Vec<u8>, bool) {
        (at, super::super::fixtures::first(), true)
    }

    fn inter(at: f64) -> (f64, Vec<u8>, bool) {
        (at, annexb(&[&[0x02, 1, 0x9a, 0x11]]), false)
    }

    #[test]
    fn short_first_publish_preserves_later_idr_boundaries_and_every_flush_sample() {
        let original: Vec<_> = (0..95)
            .map(|at| {
                if at % 30 == 0 {
                    idr(f64::from(at) * STEP)
                } else {
                    inter(f64::from(at) * STEP)
                }
            })
            .collect();
        let mut units = original.clone();
        let (first, next) = take_run(&mut units, 8, true).unwrap();
        assert_eq!(first.len(), 8);
        assert_eq!(next, Some(original[8].0));
        let (tail, next) = take_run(&mut units, 30, false).unwrap();
        assert_eq!(tail.len(), 22, "the next IDR must start its own fragment");
        assert_eq!(next, Some(original[30].0));
        assert!(units[0].2);
        let mut written = first;
        written.extend(tail);
        while let Some((run, _)) = take_run(&mut units, 30, false) {
            assert!(run[0].2);
            written.extend(run);
        }
        assert_eq!(units.len(), 5);
        assert!(units[0].2);
        written.extend(units);
        assert_eq!(written, original);
    }

    #[test]
    fn an_observed_idr_closes_a_short_tail_without_waiting_for_the_batch_limit() {
        let mut units: Vec<_> = (8..31)
            .map(|at| {
                if at == 30 {
                    idr(f64::from(at) * STEP)
                } else {
                    inter(f64::from(at) * STEP)
                }
            })
            .collect();
        let (tail, next) = take_run(&mut units, 30, false).unwrap();
        assert_eq!(tail.len(), 22);
        assert_eq!(next, Some(30.0 * STEP));
        assert_eq!(units.len(), 1);
        assert!(units[0].2);
        assert!(take_run(&mut units, 0, false).is_none());
        assert!(take_run(&mut units, 30, false).is_none());
    }

    #[test]
    fn a_second_keyframe_closes_the_first_gop() {
        let mut units = vec![idr(0.0), inter(STEP), idr(STEP * 2.0)];
        let (gop, next) = split_completed_gop(&mut units).expect("应切出封闭 GOP");
        assert_eq!(gop.len(), 2);
        assert!((next - STEP * 2.0).abs() < 1e-9);
        assert_eq!(units.len(), 1);
        assert!(split_completed_gop(&mut units).is_none());
    }

    #[test]
    fn an_open_gop_stays_buffered() {
        let mut units = vec![idr(0.0), inter(STEP)];
        assert!(split_completed_gop(&mut units).is_none());
        assert_eq!(units.len(), 2);
    }

    #[test]
    fn the_first_gop_is_playable_and_a_later_gop_appends() {
        let dir = std::env::temp_dir().join(format!("hg-frag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grow.mp4");
        let audio = AudioTrack {
            sample_rate: 44100,
            channels: 2,
            esds: None,
            samples: vec![(0.0, vec![0x11, 0x22]), (STEP * 2.0, vec![0x33])],
            end_secs: STEP * 4.0,
        };
        let mut growing = ArchiveHevc::create(&path, 64, 64, Some(&audio), 1.0).unwrap();
        let bytes = growing
            .push(
                &[idr(0.0), inter(STEP)],
                &[audio.samples[0].clone()],
                ArchiveTiming {
                    video_end_secs: STEP * 2.0,
                    audio_end_secs: Some(STEP * 2.0),
                },
            )
            .unwrap();
        let first = read_fragment_video(&path).unwrap();
        assert!(prefix_is_independently_decodable(&path));
        assert_eq!(first.pts.len(), 2);
        assert!(first.first_is_sync);
        assert_eq!(first.fragment_bases.len(), 1);
        assert!(bytes > 0);
        let again = growing
            .push(
                &[idr(STEP * 2.0), inter(STEP * 3.0)],
                &[audio.samples[1].clone()],
                ArchiveTiming {
                    video_end_secs: STEP * 4.0,
                    audio_end_secs: Some(audio.end_secs),
                },
            )
            .unwrap();
        assert!(again > bytes);
        let both = read_fragment_video(&path).unwrap();
        assert_eq!(both.pts.len(), 4);
        assert_eq!(both.fragment_bases.len(), 2);
        assert!(both.fragment_bases[1] > both.fragment_bases[0]);
        assert!((both.pts[0]).abs() < 1e-6);
        assert!((both.pts[2] - STEP * 2.0).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_is_not_an_independent_prefix() {
        let dir = std::env::temp_dir().join(format!("hg-frag-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("no.mp4");
        std::fs::write(&path, b"not a fragment").unwrap();
        assert!(!prefix_is_independently_decodable(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
