use super::*;

const HEVC: &[u8] = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const SILENT: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-64x48-no-audio.mp4");
const H264: &[u8] = include_bytes!("../../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4");

fn open(bytes: &[u8]) -> AppResult<IndexedSource> {
    IndexedSource::open(bytes.len() as u64, |offset, length, output| {
        output.clear();
        output.extend_from_slice(&bytes[offset as usize..(offset + length) as usize]);
        Ok(())
    })
}

#[test]
fn index_accepts_hevc_h264_and_silent_video_without_system_audio() {
    let index = open(HEVC).unwrap();
    assert_eq!(index.video.codec, TrackCodec::Hevc);
    assert_eq!(index.video.samples.len(), 48);
    assert!(index.audio.is_some());
    assert!((index.duration_secs - 2.0).abs() < 0.01);
    assert_eq!(open(H264).unwrap().video.codec, TrackCodec::H264);
    assert!(open(SILENT).unwrap().audio.is_none());
    assert!(open(&[]).is_err());
}

#[test]
fn indexed_seek_uses_prior_random_access_and_retains_signed_ticks() {
    let index = open(HEVC).unwrap();
    let start = index.video.seek_sample(0.7);
    assert!(
        index.video.samples[start].pts as f64 / f64::from(index.video.info.media_timescale) <= 0.7
    );
    assert!(
        index
            .video
            .info
            .stss
            .as_ref()
            .unwrap()
            .contains(&(start as u32 + 1))
    );
    assert!(
        index
            .video
            .duration_for_pts(
                index.video.samples[1].pts as f64 / f64::from(index.video.info.media_timescale)
            )
            .unwrap()
            > 0.0
    );
}

#[test]
fn movie_duration_keeps_the_audio_tail() {
    let source = open(HEVC).unwrap();
    let audio = source.audio.as_ref().unwrap();
    let mut bytes = HEVC.to_vec();
    let moov = find_box(&bytes, 0, bytes.len(), "moov").unwrap();
    for track in parse_boxes(&bytes, moov.start, moov.start + moov.size)
        .iter()
        .filter(|track| track.is("trak"))
    {
        let tkhd = find_box(&bytes, track.start, track.start + track.size, "tkhd").unwrap();
        let id_at = tkhd.start + if bytes[tkhd.start] == 1 { 20 } else { 12 };
        let id = u32::from_be_bytes(bytes[id_at..id_at + 4].try_into().unwrap());
        if id != audio.info.track_id {
            continue;
        }
        let elst = find_box(&bytes, track.start, track.start + track.size, "elst").unwrap();
        assert_eq!(bytes[elst.start], 0);
        let at = elst.start + 8;
        let duration = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
            + audio.info.movie_timescale / 2;
        bytes[at..at + 4].copy_from_slice(&duration.to_be_bytes());
    }
    assert!(open(&bytes).unwrap().duration_secs > source.duration_secs + 0.4);
}

fn properties(data: &[u8]) -> Vec<u8> {
    use hongguo_video::r#box::build_box;
    let mut output = Vec::new();
    for atom in parse_boxes(data, 0, data.len()) {
        let payload = &data[atom.start..atom.start + atom.size];
        let body = match atom.kind_str().as_str() {
            "moov" | "trak" | "mdia" | "minf" | "stbl" => properties(payload),
            "stsd" => [payload[..8].to_vec(), properties(&payload[8..])].concat(),
            "hvc1" => {
                let mut entry = payload[..VISUAL_SAMPLE_ENTRY_BYTES].to_vec();
                for child in parse_boxes(payload, VISUAL_SAMPLE_ENTRY_BYTES, payload.len())
                    .iter()
                    .filter(|child| !child.is("pasp") && !child.is("colr"))
                {
                    entry.extend(build_box(
                        &child.kind,
                        &payload[child.start..child.start + child.size],
                    ));
                }
                entry.extend(build_box(
                    b"pasp",
                    &[4u32.to_be_bytes(), 3u32.to_be_bytes()].concat(),
                ));
                entry.extend(build_box(
                    b"colr",
                    &[
                        b"nclx".as_slice(),
                        &9u16.to_be_bytes(),
                        &16u16.to_be_bytes(),
                        &9u16.to_be_bytes(),
                        &[0x80],
                    ]
                    .concat(),
                ));
                let mut mastering = Vec::new();
                for value in [10_u16, 11, 20, 21, 30, 31, 40, 41] {
                    mastering.extend(value.to_be_bytes());
                }
                mastering.extend(10_000_000_u32.to_be_bytes());
                mastering.extend(5_u32.to_be_bytes());
                entry.extend(build_box(b"mdcv", &mastering));
                entry.extend(build_box(b"clli", &[3, 232, 1, 144]));
                entry
            }
            _ => payload.to_vec(),
        };
        output.extend(build_box(&atom.kind, &body));
    }
    output
}

#[test]
fn container_rotation_sar_and_color_survive_indexing_beside_decoder_extradata() {
    let mut bytes = HEVC.to_vec();
    let tkhd = find_box(&bytes, 0, bytes.len(), "tkhd").unwrap();
    let matrix = tkhd.start + if bytes[tkhd.start] == 1 { 52 } else { 40 };
    for (offset, value) in [(0, 0i32), (4, 65_536), (12, -65_536), (16, 0)] {
        bytes[matrix + offset..matrix + offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    let bytes = properties(&bytes);
    let index = open(&bytes).unwrap();
    assert_eq!(index.video.rotation_degrees, 90);
    assert_eq!(index.video.color.sample_aspect_ratio, Some((4, 3)));
    assert_eq!(
        (
            index.video.color.primaries,
            index.video.color.transfer,
            index.video.color.matrix,
            index.video.color.full_range
        ),
        (Some(9), Some(16), Some(9), Some(true))
    );
    assert!(!index.video.extradata.is_empty());
    let mastering = index
        .video
        .color
        .mastering_display
        .expect("container mastering metadata");
    assert_eq!(mastering.primaries, [[30, 31], [10, 11], [20, 21]]);
    assert_eq!(mastering.max_luminance, 10_000_000);
    assert_eq!(
        index
            .video
            .color
            .content_light
            .unwrap()
            .max_content_light_level,
        1000
    );
}
