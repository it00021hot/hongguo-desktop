use hongguo_hevc::Decoder;
use hongguo_media_types::NativeFrame;

const MAIN: &[u8] = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const MAIN10: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");
const START_CODE: [u8; 4] = [0, 0, 0, 1];

#[test]
fn full_range_planes_keep_their_metadata_and_code_values() {
    let frames = decode(include_bytes!("fixtures/hevc-full-range.mp4"), 2);
    assert!(!frames.is_empty());
    assert!(
        frames
            .iter()
            .all(|frame| frame.color.full_range == Some(true))
    );
    let luma: Vec<u8> = frames
        .iter()
        .flat_map(|frame| frame.planes[0].data.iter().copied())
        .collect();
    assert!(luma.iter().any(|value| *value < 16));
    assert_eq!(fingerprint(&frames), 0xe3b2_ac82_0a17_93b7);
}

fn decode(data: &[u8], threads: u32) -> Vec<NativeFrame> {
    let media = hongguo_media::demux::demux_bytes(data).unwrap();
    let track = &media.video_track().unwrap().info;
    let (parameters, length_size) =
        hongguo_media::hevc::param_nalus_from_head(data, track).unwrap();
    let parameters: Vec<u8> = parameters
        .into_iter()
        .flat_map(|nal| START_CODE.into_iter().chain(nal))
        .collect();
    let mut decoder = Decoder::new(threads, track.media_timescale).unwrap();
    let times = track.media_sample_timeline().unwrap();
    let mut frames = Vec::new();
    for (index, ((offset, size), time)) in track.samples.iter().zip(times).enumerate() {
        let mut packet = if index == 0 {
            parameters.clone()
        } else {
            Vec::new()
        };
        packet.extend(hongguo_media::hevc::to_annexb(
            &data[*offset as usize..(*offset + *size) as usize],
            length_size,
        ));
        frames.extend(decoder.decode(&packet, time.pts).unwrap());
    }
    frames.extend(decoder.drain().unwrap());
    assert!(decoder.drain().unwrap().is_empty());
    assert!(decoder.decode(&START_CODE, 0).is_err());
    assert_eq!(
        frames.len(),
        track.samples.len(),
        "drain must preserve reordered tail frames"
    );
    frames
}

fn fingerprint(frames: &[NativeFrame]) -> u64 {
    frames
        .iter()
        .flat_map(|frame| &frame.planes)
        .flat_map(|plane| plane.data.iter())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &value| {
            (hash ^ u64::from(value)).wrapping_mul(0x100_0000_01b3)
        })
}

#[test]
fn cropped_main_frames_match_the_existing_full_resolution_golden() {
    for threads in [1, 4] {
        let frames = decode(MAIN, threads);
        assert!(
            frames
                .iter()
                .all(|frame| frame.width == 136 && frame.height == 76 && frame.bit_depth == 8)
        );
        assert_eq!(fingerprint(&frames), 0xe3b2_ac82_0a17_93b7);
    }
}

#[test]
fn main10_preserves_lower_bits_and_reorders_signed_presentation_times() {
    let single = decode(MAIN10, 1);
    let threaded = decode(MAIN10, 4);
    assert_eq!(fingerprint(&single), fingerprint(&threaded));
    assert!(
        single
            .iter()
            .all(|frame| frame.bit_depth == 10 && frame.width == 128 && frame.height == 72)
    );
    assert!(
        single
            .iter()
            .flat_map(|frame| &frame.planes)
            .flat_map(|plane| plane.data.as_chunks::<2>().0)
            .any(|sample| { u16::from_le_bytes(*sample) & 3 != 0 })
    );
    assert!(
        single
            .windows(2)
            .all(|pair| pair[0].pts.unwrap() <= pair[1].pts.unwrap())
    );
}

#[test]
fn invalid_configuration_and_empty_packets_are_rejected() {
    assert!(Decoder::new(0, 90_000).is_err());
    assert!(Decoder::new(1, 0).is_err());
    assert!(Decoder::new(65, 90_000).is_err());
    assert!(Decoder::new(1, 90_000).unwrap().decode(&[], 0).is_err());
}
