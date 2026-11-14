use super::*;

const MAIN: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const MAIN10: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");

pub(super) fn pictures(data: &[u8]) -> Vec<Picture> {
    let media = hongguo_media::demux::demux_bytes(data).unwrap();
    let track = &media.video_track().unwrap().info;
    let (parameters, length) = hongguo_media::hevc::param_nalus_from_head(data, track).unwrap();
    let parameters: Vec<u8> = parameters
        .into_iter()
        .flat_map(|nal| [0, 0, 0, 1].into_iter().chain(nal))
        .collect();
    let mut parser = Parser::new();
    parser.parameters(&parameters).unwrap();
    let mut pictures = Vec::new();
    for &(offset, size) in &track.samples {
        let packet = hongguo_media::hevc::to_annexb(
            &data[offset as usize..(offset + size) as usize],
            length,
        );
        pictures.push(parser.picture(&packet).unwrap().unwrap());
    }
    pictures
}

#[test]
fn main_and_main10_keep_coded_dimensions_crop_depth_and_reordered_references() {
    for (sample, visible, depth) in [(MAIN, (136, 76), 8), (MAIN10, (128, 72), 10)] {
        let pictures = pictures(sample);
        assert!(!pictures.is_empty());
        assert!(pictures[0].reset);
        let mut decoded = Vec::new();
        for (index, picture) in pictures.iter().enumerate() {
            let descriptor = &picture.descriptor;
            assert_eq!((descriptor.crop.width, descriptor.crop.height), visible);
            assert_eq!(descriptor.bit_depth, depth);
            assert!(
                descriptor.width >= descriptor.crop.width
                    && descriptor.height >= descriptor.crop.height
            );
            for reference in &picture.references {
                assert!(
                    decoded.iter().any(|&poc| reference.matches(poc)),
                    "missing reference at picture {index}"
                );
            }
            decoded.push(picture.poc);
        }
        assert!(
            pictures.windows(2).any(|pair| pair[0].poc > pair[1].poc),
            "fixture must exercise B-picture reordering"
        );
    }
}

#[test]
fn dxva_parameters_match_packed_sdk_layout_and_keep_main10_flags() {
    for (sample, depth) in [(MAIN, 8), (MAIN10, 10)] {
        let pictures = pictures(sample);
        let mut surfaces = Vec::new();
        for (index, picture) in pictures.iter().enumerate() {
            let active: Vec<_> = surfaces
                .iter()
                .filter(|surface: &&ReferenceSurface| {
                    picture
                        .references
                        .iter()
                        .any(|reference| reference.matches(surface.poc))
                })
                .copied()
                .collect();
            let params = picture
                .parameters((index % 32) as u8, &active, index as u32 + 1)
                .unwrap();
            assert_eq!(params.len(), 232);
            let flags = u16::from_le_bytes(params[4..6].try_into().unwrap());
            assert_eq!((flags >> 3) & 7, depth - 8);
            assert_eq!((flags >> 6) & 7, depth - 8);
            assert_eq!(params[6], (index % 32) as u8);
            if picture.sps.st_rps.is_empty() {
                assert_eq!(params[19], 0, "no SPS RPS exists to predict from");
            }
            assert_eq!(
                i32::from_le_bytes(params[120..124].try_into().unwrap()),
                picture.poc
            );
            assert_eq!(
                u32::from_le_bytes(params[228..232].try_into().unwrap()),
                index as u32 + 1
            );
            for offset in [200, 208, 216] {
                assert!(
                    params[offset..offset + 8]
                        .iter()
                        .all(|value| *value == 0xff || (*value as usize) < active.len())
                );
            }
            let padded = picture.bitstream.len().next_multiple_of(128);
            let slices = picture.slice_control(padded).unwrap();
            assert_eq!(slices.len(), picture.slices.len() * 10);
            let last = &slices[slices.len() - 10..];
            let start = u32::from_le_bytes(last[..4].try_into().unwrap());
            let count = u32::from_le_bytes(last[4..8].try_into().unwrap());
            assert_eq!((start + count) as usize, padded);
            if let Some(matrix) = picture.quantization() {
                assert_eq!(matrix.len(), 1000);
            }
            surfaces.push(ReferenceSurface {
                poc: picture.poc,
                slot: (index % 32) as u8,
                long_term: false,
            });
        }
    }
}

#[test]
fn invalid_empty_oversized_and_continuation_access_units_are_rejected() {
    let mut parser = Parser::new();
    assert!(parser.picture(&[]).is_err());
    assert!(parser.picture(&[0, 0, 1, 0xff, 0]).is_err());
    assert!(parser.picture(&vec![0; MAX_ACCESS_UNIT_BYTES + 1]).is_err());
    assert!(parser.picture(&[0, 0, 1, 2, 1, 0x80]).is_err());
    assert!(parser.format().is_err());
    assert!(parser.picture(&[0, 0, 1, 0x46, 1, 0x50]).unwrap().is_none());
}

#[test]
fn mastering_and_light_metadata_survive_sei_and_truncation_is_rejected() {
    let mut bytes = vec![137, 24];
    for value in [10_u16, 11, 20, 21, 30, 31, 40, 41] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend(10_000_000_u32.to_be_bytes());
    bytes.extend(5_u32.to_be_bytes());
    bytes.extend([144, 4, 3, 232, 1, 144, 0x80]);
    let mut color = ColorMetadata::default();
    metadata::sei(&bytes, &mut color).unwrap();
    let display = color.mastering_display.unwrap();
    assert_eq!(display.primaries, [[30, 31], [10, 11], [20, 21]]);
    assert_eq!(display.max_luminance, 10_000_000);
    assert_eq!(color.content_light.unwrap().max_content_light_level, 1000);
    for length in [1, 3, 10, 25] {
        assert!(metadata::sei(&bytes[..length], &mut ColorMetadata::default()).is_err());
    }
}

#[test]
fn predicted_rps_uses_the_referenced_sps_set_size() {
    let sps = Sps {
        log2_max_poc_lsb: 4,
        st_rps: vec![rusty_h265::ps::ShortTermRps {
            neg: vec![(-1, true)],
            pos: vec![(1, true), (2, true)],
        }],
        ..Default::default()
    };
    let pps = Pps::default();
    let nal = NalHeader::parse(&[2, 1]).unwrap();
    // first slice, PPS 0, I slice, POC 3, inline predicted RPS, delta index 0.
    assert_eq!(
        parameters::reference_rps_count(&[0b1101_1001, 0b1011_0000], &sps, &pps, nal).unwrap(),
        3
    );
}

#[test]
fn bounded_annexb_rejects_many_nals_without_unbounded_allocations() {
    let bytes = [0, 0, 1, 0x46, 1, 0x50].repeat(4097);
    assert!(Parser::new().parameters(&bytes).is_err());
    assert!(Parser::new().parameters(&[0, 0, 1, 0, 0, 1]).is_err());
}

#[test]
fn dependent_segments_share_the_preceding_independent_dxva_slice() {
    let first = pictures(MAIN).remove(0);
    let mut pps = (*first.pps).clone();
    pps.dependent_slice_segments_enabled = true;
    let mut parser = Parser::new();
    parser.sps.insert(first.sps.id, first.sps.clone());
    parser.pps.insert(pps.id, Arc::new(pps.clone()));
    let mut bits = vec![false];
    if first.nal.nal_type.is_irap() {
        bits.push(first.header.no_output_of_prior_pics);
    }
    ue(&mut bits, pps.id as u32);
    bits.push(true);
    uint(
        &mut bits,
        1,
        32 - (first.sps.pic_size_in_ctbs - 1).leading_zeros(),
    );
    if pps.tiles_enabled || pps.entropy_coding_sync_enabled {
        ue(&mut bits, 0);
    }
    if pps.slice_segment_header_extension_present {
        ue(&mut bits, 0);
    }
    bits.push(true);
    let rbsp = pack(bits);
    let mut packet = first.bitstream.clone();
    packet.extend([
        0,
        0,
        1,
        first.nal.nal_type.id() << 1,
        first.nal.temporal_id + 1,
    ]);
    rusty_h265::nal::escape_into(&rbsp, &mut packet);
    let picture = parser.picture(&packet).unwrap().unwrap();
    assert_eq!(picture.slices.len(), 1);
    assert_eq!(picture.slices[0].length as usize, picture.bitstream.len());
}

#[test]
fn hdr_vui_keeps_pq_hlg_sar_chroma_location_and_range() {
    for transfer in [16, 18] {
        let mut bits = vec![true];
        uint(&mut bits, 14, 8);
        bits.extend([false, true]);
        uint(&mut bits, 5, 3);
        bits.extend([false, true]);
        for value in [9, transfer, 9] {
            uint(&mut bits, value, 8);
        }
        bits.push(true);
        ue(&mut bits, 2);
        ue(&mut bits, 2);
        bits.extend([false, false]);
        let bytes = pack(bits);
        let metadata = metadata::vui(&mut BitReader::new(&bytes)).unwrap();
        assert_eq!(metadata.primaries, Some(9));
        assert_eq!(metadata.transfer, Some(transfer as u8));
        assert_eq!(metadata.matrix, Some(9));
        assert_eq!(metadata.full_range, Some(false));
        assert_eq!(metadata.chroma_location, Some(3));
        assert_eq!(metadata.sample_aspect_ratio, Some((4, 3)));
    }
}

#[test]
fn bootstrap_selects_the_sps_referenced_by_the_first_access_unit() {
    let main = pictures(MAIN).remove(0);
    let main10 = pictures(MAIN10).remove(0);
    let mut sps = (*main10.sps).clone();
    sps.id = 1;
    let mut pps = (*main10.pps).clone();
    pps.sps_id = 1;
    let mut parser = Parser::new();
    parser.parameters(&[]).unwrap();
    parser.sps.insert(0, main.sps);
    parser.sps.insert(1, Arc::new(sps));
    parser.pps.insert(pps.id, Arc::new(pps));
    parser.parameters(&main10.bitstream).unwrap();
    assert_eq!(parser.format().unwrap().bit_depth, 10);
    assert_eq!(parser.format().unwrap().crop.width, 128);
}

fn uint(bits: &mut Vec<bool>, value: u32, count: u32) {
    for shift in (0..count).rev() {
        bits.push(value & (1 << shift) != 0);
    }
}
fn ue(bits: &mut Vec<bool>, value: u32) {
    let code = value + 1;
    let count = 32 - code.leading_zeros();
    bits.extend(std::iter::repeat_n(false, (count - 1) as usize));
    uint(bits, code, count);
}
fn pack(mut bits: Vec<bool>) -> Vec<u8> {
    bits.resize(bits.len().next_multiple_of(8), false);
    bits.as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| {
            chunk
                .iter()
                .fold(0, |value, bit| value * 2 + u8::from(*bit))
        })
        .collect()
}
