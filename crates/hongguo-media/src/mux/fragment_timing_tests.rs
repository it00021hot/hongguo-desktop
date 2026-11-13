use super::*;

fn first_durations(path: &Path) -> Vec<u32> {
    let bytes = std::fs::read(path).unwrap();
    let moof = parse_boxes(&bytes, 0, bytes.len())
        .into_iter()
        .find(|header| header.is("moof"))
        .unwrap();
    parse_boxes(&bytes, moof.start, moof.start + moof.size)
        .into_iter()
        .filter(|header| header.is("traf"))
        .map(|traf| {
            let trun = parse_boxes(&bytes, traf.start, traf.start + traf.size)
                .into_iter()
                .find(|header| header.is("trun"))
                .unwrap();
            read_u32(&bytes[trun.start..trun.start + trun.size], 12).unwrap()
        })
        .collect()
}

#[test]
fn singleton_video_and_he_aac_keep_their_real_sample_durations() {
    let path = std::env::temp_dir().join(format!("hg-singleton-timing-{}.mp4", std::process::id()));
    let audio = AudioTrack {
        sample_rate: 44_100,
        channels: 2,
        esds: None,
        samples: vec![(0.0, vec![1]), (2048.0 / 44_100.0, vec![2])],
        end_secs: 4096.0 / 44_100.0,
    };
    let mut movie = GrowingH264::create(&path, 64, 64, Some(&audio), 1.0).unwrap();
    movie
        .push(
            &[super::tests::idr(0.0)],
            &audio.samples[..1],
            FragmentTiming {
                video_end_secs: 1.0 / 30.0,
                audio_end_secs: Some(audio.samples[1].0),
            },
        )
        .unwrap();
    assert_eq!(first_durations(&path), [3000, 4180]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn fractional_aac_durations_round_endpoints_without_accumulating_drift() {
    let step = 1024.0 / 22_050.0;
    let packets: Vec<_> = (0..3)
        .map(|index| (f64::from(index) * step, vec![index as u8], true))
        .collect();
    let samples = time_samples(&packets, false, 3.0 * step).unwrap();
    assert_eq!(
        samples.iter().map(|sample| sample.dur).collect::<Vec<_>>(),
        [4180, 4179, 4180]
    );
    assert_eq!(
        samples
            .iter()
            .map(|sample| u64::from(sample.dur))
            .sum::<u64>(),
        ticks(3.0 * step).unwrap()
    );
}

#[test]
fn reordered_video_preserves_decode_durations_and_signed_composition_offsets() {
    let frames = [
        super::tests::idr(0.0),
        super::tests::idr(2.0 / 30.0),
        super::tests::idr(1.0 / 30.0),
    ];
    let samples = time_samples(&frames, true, 3.0 / 30.0).unwrap();
    assert_eq!(
        samples.iter().map(|sample| sample.dur).collect::<Vec<_>>(),
        [3000; 3]
    );
    assert_eq!(
        samples.iter().map(|sample| sample.cto).collect::<Vec<_>>(),
        [0, 3000, -3000]
    );
}

#[test]
fn fragment_tails_require_positive_finite_real_boundaries() {
    let frame = [super::tests::idr(1.0)];
    for end in [f64::NAN, f64::INFINITY, 0.0, 1.0] {
        assert!(time_samples(&frame, true, end).is_err());
    }
    assert!(time_samples(&[], true, 0.0).unwrap().is_empty());
    assert!(time_samples(&frame, true, 1.0 + f64::from(u32::MAX) / 90_000.0 + 1.0).is_err());
}

#[test]
fn source_track_end_uses_the_final_stts_delta_and_presentation_order() {
    use hongguo_video::sample_table::TrackInfo;
    let mut track = TrackInfo {
        media_timescale: 22_050,
        sample_count: 3,
        samples: vec![(0, 1); 3],
        stts: vec![(3, 1024)],
        ..Default::default()
    };
    assert!((crate::mux::track_end_secs(&track).unwrap() - 3072.0 / 22_050.0).abs() < 1e-12);
    track.media_timescale = 30;
    track.stts = vec![(3, 1)];
    track.ctts = vec![(1, 0), (1, 1), (1, -1)];
    assert!((crate::mux::track_end_secs(&track).unwrap() - 0.1).abs() < 1e-12);
    track.stts.clear();
    assert!(crate::mux::track_end_secs(&track).is_err());
}
