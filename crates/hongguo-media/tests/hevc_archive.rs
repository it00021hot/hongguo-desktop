use std::path::{Path, PathBuf};

use hongguo_media::{demux, hevc, mux};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hongguo-softdec/tests/fixtures")
        .join(name)
}

fn frames(path: &Path) -> Vec<mux::AnnexbFrame> {
    let demux = demux::demux_file(path).unwrap();
    let track = &demux.video_track().unwrap().info;
    let (sets, length) = hevc::read_parameter_set_nalus(path, track).unwrap();
    let mut parameters = Vec::new();
    for nal in sets {
        parameters.extend([0, 0, 0, 1]);
        parameters.extend(nal);
    }
    track
        .samples
        .iter()
        .zip(track.sample_pts())
        .enumerate()
        .map(|(index, (&(offset, size), pts))| {
            let raw = hongguo_media::transcode::read_range(path, offset, size).unwrap();
            let annexb = hevc::to_annexb(&raw, length);
            let key = track
                .stss
                .as_ref()
                .is_none_or(|samples| samples.contains(&(index as u32 + 1)));
            let data = if key {
                [parameters.clone(), annexb].concat()
            } else {
                annexb
            };
            (pts, data, key)
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct Decoded {
    width: usize,
    height: usize,
    bit_depth: u8,
    yuv: Vec<u8>,
}

fn decoded_frames(path: &Path) -> Vec<Decoded> {
    let mut decoder = rusty_h265::Decoder::new();
    for (pts, bytes, _) in frames(path) {
        decoder
            .push_annexb(&bytes, Some((pts * 1000.0) as i64))
            .unwrap();
    }
    decoder.flush();
    let mut decoded = Vec::new();
    while let Ok(frame) = decoder.next_frame() {
        let mut yuv = Vec::new();
        frame.write_yuv(&mut yuv);
        decoded.push(Decoded {
            width: frame.width,
            height: frame.height,
            bit_depth: frame.bit_depth(),
            yuv,
        });
    }
    decoded
}

#[test]
fn real_hevc_main_and_main10_roundtrip_with_every_picture_decodable() {
    let dir = std::env::temp_dir().join(format!("hg-hevc-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "hevc-main-136x76-aac.mp4",
        "hevc-main10-128x72-hev1-aac.mp4",
    ] {
        let source = fixture(name);
        let source_movie = demux::demux_file(&source).unwrap();
        let video = &source_movie.video_track().unwrap().info;
        let encoded = frames(&source);
        let output = dir.join(name);
        mux::with_audio(
            &source,
            &source_movie,
            &output,
            &encoded,
            video.width as usize,
            video.height as usize,
        )
        .unwrap();
        let movie = demux::demux_file(&output).unwrap();
        assert_eq!(movie.video_track().unwrap().info.codec, "hvc1");
        assert_eq!(
            movie.video_track().unwrap().info.samples.len(),
            encoded.len()
        );
        let decoded = decoded_frames(&output);
        assert_eq!(decoded.len(), encoded.len());
        assert_eq!(decoded, decoded_frames(&source));
        let source_audio = mux::collect(&source, &source_movie).unwrap().unwrap();
        let output_audio = mux::collect(&output, &movie).unwrap().unwrap();
        let expected: Vec<_> = source_audio
            .samples
            .iter()
            .filter(|(pts, _)| *pts >= encoded[0].0)
            .map(|(_, bytes)| bytes)
            .collect();
        assert_eq!(
            output_audio
                .samples
                .iter()
                .map(|(_, bytes)| bytes)
                .collect::<Vec<_>>(),
            expected
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn original_hevc_merge_copies_every_video_and_aac_sample_without_reencoding() {
    let source = fixture("hevc-main-136x76-aac.mp4");
    let output =
        std::env::temp_dir().join(format!("hg-hevc-original-copy-{}.mp4", std::process::id()));
    let original = demux::demux_file(&source).unwrap();
    let (_, count) =
        hongguo_media::remux::concat_copy(&[source.clone(), source.clone()], &output).unwrap();
    assert_eq!(count, 2);
    let merged = demux::demux_file(&output).unwrap();
    for (before, after) in original.tracks.iter().zip(&merged.tracks) {
        assert_eq!(after.info.codec, before.info.codec);
        assert_eq!(after.info.samples.len(), before.info.samples.len() * 2);
        let expected: Vec<_> = before
            .info
            .samples
            .iter()
            .map(|&(offset, length)| {
                hongguo_media::transcode::read_range(&source, offset, length).unwrap()
            })
            .collect();
        let actual: Vec<_> = after
            .info
            .samples
            .iter()
            .map(|&(offset, length)| {
                hongguo_media::transcode::read_range(&output, offset, length).unwrap()
            })
            .collect();
        assert_eq!(actual, [expected.clone(), expected].concat());
    }
    std::fs::remove_file(output).unwrap();
}

#[test]
fn fragmented_hevc_archive_can_be_merged_with_all_samples() {
    let source = fixture("hevc-main-136x76-aac.mp4");
    let dir = std::env::temp_dir().join(format!("hg-hevc-fragment-merge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("archive.mp4");
    let output = dir.join("merged.mp4");
    let movie = demux::demux_file(&source).unwrap();
    let video = &movie.video_track().unwrap().info;
    let audio = mux::collect(&source, &movie).unwrap().unwrap();
    let units = frames(&source);
    let end = mux::track_end_secs(video).unwrap();
    let mut writer = mux::ArchiveHevc::create(
        &path,
        video.width as usize,
        video.height as usize,
        Some(&audio),
        end,
    )
    .unwrap();
    writer
        .push(
            &units,
            &audio.samples,
            mux::ArchiveTiming {
                video_end_secs: end,
                audio_end_secs: Some(audio.end_secs),
            },
        )
        .unwrap();
    writer.finish().unwrap();
    let archived = demux::demux_file(&path).unwrap();
    let archived_video = &archived.video_track().unwrap().info;
    for (actual, (expected, _, _)) in archived_video.sample_pts().iter().zip(&units) {
        assert!((actual - expected).abs() < 1.0 / 90_000.0);
    }
    let archived_audio = mux::collect(&path, &archived).unwrap().unwrap();
    assert_eq!(archived_audio.samples.len(), audio.samples.len());
    for ((actual_pts, actual), (expected_pts, expected)) in
        archived_audio.samples.iter().zip(&audio.samples)
    {
        assert!((actual_pts - expected_pts).abs() < 1.0 / 90_000.0);
        assert_eq!(actual, expected);
    }
    hongguo_media::remux::concat_copy(&[path.clone(), path], &output).unwrap();
    let merged = demux::demux_file(&output).unwrap();
    assert_eq!(
        merged.video_track().unwrap().info.samples.len(),
        units.len() * 2
    );
    assert_eq!(
        merged.audio_track().unwrap().info.samples.len(),
        audio.samples.len() * 2
    );
    std::fs::remove_dir_all(dir).unwrap();
}
