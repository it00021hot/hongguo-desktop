use super::*;

const FRAME_LIMIT: usize = 60;
const FPS: f64 = 30.0;

fn packets(data: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 < data.len() {
        let size = if data[index..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if data[index..].starts_with(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        starts.push((index, size));
        index += size;
    }
    let mut samples = Vec::new();
    let mut pending = Vec::new();
    let mut sps = None;
    for (index, &(at, prefix)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map_or(data.len(), |(next, _)| *next);
        let nal = &data[at + prefix..end];
        assert!(nal.len() >= 2);
        let kind = (nal[1] >> 3) & 31;
        if kind == 15 && sps.is_none() {
            sps = Some(nal.to_vec());
        }
        pending.extend((nal.len() as u32).to_be_bytes());
        pending.extend(nal);
        if kind <= 11 {
            samples.push(std::mem::take(&mut pending));
            if samples.len() == FRAME_LIMIT {
                break;
            }
        }
    }
    let sps = sps.expect("真实流含 SPS");
    let mut config = vec![0, 6, 1, 15, 0, 1];
    config.extend(u16::try_from(sps.len()).unwrap().to_be_bytes());
    config.extend(sps);
    (samples, config)
}

#[test]
#[ignore = "显式 HONGGUO_BYTEVC2_ARCHIVE_RAW 指定自有真实 Annex-B；不读取用户 profile"]
fn real_bytevc2_raw_frames_are_hevc_encoded_and_mergeable() {
    let source = std::env::var_os("HONGGUO_BYTEVC2_ARCHIVE_RAW").expect("显式真实样本路径");
    let data = std::fs::read(source).unwrap();
    let (samples, config) = packets(&data);
    assert_eq!(samples.len(), FRAME_LIMIT);
    let mut track = hongguo_bytevc2::Bytevc2Track {
        info: Default::default(),
        config,
    };
    track.info.media_timescale = 90_000;
    track.info.stts = vec![(FRAME_LIMIT as u32, 3000)];
    track.info.samples = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| (index as u64, sample.len() as u64))
        .collect();
    let mut encoder = YuvEncoder::new(Some((128, 72)), FPS);
    let mut count = 0;
    hongguo_bytevc2::decode_track_vvdec(
        &track,
        0,
        |offset, size, out| {
            let sample = &samples[offset as usize];
            assert_eq!(sample.len() as u64, size);
            out.clear();
            out.extend(sample);
            Ok(())
        },
        &mut |frame| {
            encoder.push(&hongguo_media_platform_api::I420View {
                y: &frame.y,
                u: &frame.u,
                v: &frame.v,
                width: frame.width,
                height: frame.height,
                pts: count as f64 / FPS,
                dur: 1.0 / FPS,
            })?;
            count += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count, FRAME_LIMIT);
    let encoded = encoder.finish().unwrap();
    assert_eq!(encoded.units.len(), FRAME_LIMIT);
    let dir = std::env::temp_dir().join(format!("hg-real-bvc2-hevc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("archive.mp4");
    let merged = dir.join("merged.mp4");
    let duration = count as f64 / FPS;
    let mut writer =
        ArchiveHevc::create(&archive, encoded.width, encoded.height, None, duration).unwrap();
    writer
        .push(
            &encoded.units,
            &[],
            hongguo_media::mux::ArchiveTiming {
                video_end_secs: duration,
                audio_end_secs: None,
            },
        )
        .unwrap();
    writer.finish().unwrap();
    let movie = hongguo_media::demux::demux_file(&archive).unwrap();
    let video = &movie.video_track().unwrap().info;
    assert_eq!(video.codec, "hvc1");
    assert_eq!(video.samples.len(), FRAME_LIMIT);
    assert!(
        (video.media_duration as f64 / f64::from(video.media_timescale) - duration).abs() < 0.001
    );
    hongguo_media::remux::concat_copy(&[archive.clone(), archive], &merged).unwrap();
    let movie = hongguo_media::demux::demux_file(&merged).unwrap();
    assert_eq!(
        movie.video_track().unwrap().info.samples.len(),
        FRAME_LIMIT * 2
    );
    assert!(
        movie.audio_track().is_none(),
        "真实 raw 没有 AAC，不构造假音轨"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
