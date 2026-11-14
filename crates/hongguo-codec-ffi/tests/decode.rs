use hongguo_codec_ffi::{
    AudioDecoder, AudioFormat, Backend, Codec, DecoderConfig, Packet, VideoDecoder, VideoFrame,
};
use hongguo_video::{
    r#box::{find_box, parse_boxes},
    sample_table::{TrackInfo, collect_tracks},
};

const MAIN10: &[u8] =
    include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");
const MAIN: &[u8] = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const H264: &[u8] = include_bytes!("../../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4");

fn tracks(data: &[u8]) -> Vec<TrackInfo> {
    collect_tracks(data).unwrap()
}

fn video_config(data: &[u8], track: &TrackInfo) -> Vec<u8> {
    let start = track.stbl_offset;
    let end = track.stbl_offset + track.stbl_size;
    let stsd = find_box(data, start, end, "stsd").unwrap();
    const FULL_BOX_AND_ENTRY_COUNT: usize = 8;
    const VISUAL_SAMPLE_ENTRY: usize = 78;
    let entry = parse_boxes(
        data,
        stsd.start + FULL_BOX_AND_ENTRY_COUNT,
        stsd.start + stsd.size,
    )
    .remove(0);
    let config = find_box(
        data,
        entry.start + VISUAL_SAMPLE_ENTRY,
        entry.start + entry.size,
        if track.codec == "avc1" {
            "avcC"
        } else {
            "hvcC"
        },
    )
    .unwrap();
    data[config.start..config.start + config.size].to_vec()
}

fn decode_video(data: &[u8], codec: Codec) -> Vec<hongguo_media_types::NativeFrame> {
    decode_backend(data, codec, Backend::Software)
        .into_iter()
        .map(|frame| frame.cpu().unwrap())
        .collect()
}

fn decode_backend(data: &[u8], codec: Codec, backend: Backend) -> Vec<VideoFrame> {
    let track = tracks(data)
        .into_iter()
        .find(|track| track.is_video)
        .unwrap();
    assert!(matches!(backend, Backend::Software | Backend::VideoToolbox));
    // Neither test backend borrows an external device or Java surface pointer.
    let mut decoder = unsafe {
        VideoDecoder::new(DecoderConfig {
            codec,
            backend,
            extradata: video_config(data, &track),
            time_base: (1, track.media_timescale as i32),
            threads: 1,
        })
    }
    .unwrap();
    let times = track.media_sample_timeline().unwrap();
    let mut frames = Vec::new();
    let mut receive = |decoder: &mut VideoDecoder| {
        while let Some(frame) = decoder.receive().unwrap() {
            frames.push(frame);
        }
    };
    for ((offset, size), time) in track.samples.iter().zip(times) {
        let packet = Packet {
            data: &data[*offset as usize..(*offset + *size) as usize],
            pts: Some(time.pts),
            dts: Some(time.dts),
            duration: time.duration as i64,
        };
        if !decoder.send(&packet).unwrap() {
            receive(&mut decoder);
            assert!(decoder.send(&packet).unwrap());
        }
        receive(&mut decoder);
    }
    assert!(decoder.drain().unwrap());
    receive(&mut decoder);
    assert_eq!(frames.len(), track.samples.len());
    frames
}

#[test]
fn ffmpeg_decodes_b_frames_with_signed_pts_and_native_ten_bit_samples() {
    let frames = decode_video(MAIN10, Codec::Hevc);
    assert!(!frames.is_empty());
    assert!(
        frames
            .iter()
            .all(|frame| frame.bit_depth == 10 && frame.width == 128 && frame.height == 72)
    );
    assert!(
        frames
            .iter()
            .flat_map(|frame| frame.planes[0].data.as_chunks::<2>().0.iter())
            .any(|sample| u16::from_le_bytes([sample[0], sample[1]]) & 3 != 0)
    );
    assert!(
        frames
            .windows(2)
            .all(|pair| pair[0].pts.unwrap() <= pair[1].pts.unwrap())
    );
}

#[test]
fn ffmpeg_decodes_hevc_main_and_h264_without_system_codec() {
    let frames = decode_video(MAIN, Codec::Hevc);
    assert_eq!(frames[0].width, 136);
    // Independently generated FFmpeg 7.1.1 full-resolution display-order
    // I420 golden, shared with the pre-existing softdec fixture regression.
    let hash = frames
        .iter()
        .flat_map(|frame| &frame.planes)
        .flat_map(|plane| plane.data.iter())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &value| {
            (hash ^ u64::from(value)).wrapping_mul(0x100_0000_01b3)
        });
    assert_eq!(hash, 0xe3b2_ac82_0a17_93b7);
    assert_eq!(decode_video(H264, Codec::H264)[0].width, 64);
}

fn pcm_frames(data: &[u8], rate: f64) -> (usize, f32) {
    let track = tracks(data)
        .into_iter()
        .find(|track| track.codec == "mp4a")
        .unwrap();
    let format = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
    };
    let mut decoder =
        AudioDecoder::new(&track.decoder_config, track.media_timescale, format, rate).unwrap();
    let times = track.media_sample_timeline().unwrap();
    let mut frames = 0;
    let mut peak = 0_f32;
    let mut receive = |decoder: &mut AudioDecoder| {
        while let Some(block) = decoder.receive().unwrap() {
            assert!(block.samples.iter().all(|value| value.is_finite()));
            peak = block
                .samples
                .iter()
                .fold(peak, |peak, value| peak.max(value.abs()));
            assert!(block.media_start_secs.is_some());
            frames += block.samples.len() / usize::from(format.channels);
        }
    };
    for ((offset, size), time) in track.samples.iter().zip(times) {
        let packet = &data[*offset as usize..(*offset + *size) as usize];
        if !decoder.send(packet, Some(time.pts)).unwrap() {
            receive(&mut decoder);
            assert!(decoder.send(packet, Some(time.pts)).unwrap());
        }
        receive(&mut decoder);
    }
    assert!(decoder.drain().unwrap());
    receive(&mut decoder);
    (frames, peak)
}

#[test]
fn aac_resampling_and_every_product_rate_produce_finite_pcm_and_drain_tail() {
    let (normal, peak) = pcm_frames(MAIN, 1.0);
    assert!(normal > 0 && peak > 0.01);
    for rate in [0.75, 1.25, 1.5, 2.0, 3.0] {
        let (frames, peak) = pcm_frames(MAIN, rate);
        assert!(frames > 0 && peak > 0.01);
        let relative = ((frames as f64 * rate) / normal as f64 - 1.0).abs();
        assert!(
            relative < 0.08,
            "rate={rate}, frames={frames}, normal={normal}"
        );
    }
}

#[test]
fn audio_timestamp_gaps_remain_gaps_after_atempo() {
    let track = tracks(MAIN)
        .into_iter()
        .find(|track| track.codec == "mp4a")
        .unwrap();
    let times = track.media_sample_timeline().unwrap();
    let mut decoder = AudioDecoder::new(
        &track.decoder_config,
        track.media_timescale,
        AudioFormat {
            sample_rate: 48_000,
            channels: 2,
        },
        1.5,
    )
    .unwrap();
    let midpoint = track.samples.len() / 2;
    let mut starts = Vec::new();
    for (index, ((offset, size), time)) in track.samples.iter().zip(times).enumerate() {
        let pts = time.pts
            + if index >= midpoint {
                i64::from(track.media_timescale)
            } else {
                0
            };
        assert!(
            decoder
                .send(
                    &MAIN[*offset as usize..(*offset + *size) as usize],
                    Some(pts)
                )
                .unwrap()
        );
        while let Some(block) = decoder.receive().unwrap() {
            starts.push(block.media_start_secs.unwrap());
        }
    }
    assert!(decoder.drain().unwrap());
    while let Some(block) = decoder.receive().unwrap() {
        starts.push(block.media_start_secs.unwrap());
    }
    assert!(
        starts.windows(2).any(|pair| pair[1] - pair[0] > 0.9),
        "PCM collapsed the one-second source gap: {starts:?}"
    );
}

#[test]
fn aac_reset_after_eof_discards_old_tempo_and_reanchors_new_pcm() {
    let track = tracks(MAIN)
        .into_iter()
        .find(|track| track.codec == "mp4a")
        .unwrap();
    let times = track.media_sample_timeline().unwrap();
    let format = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
    };
    let mut decoder =
        AudioDecoder::new(&track.decoder_config, track.media_timescale, format, 0.75).unwrap();
    for (round, rate) in [0.75, 3.0, 0.375, 6.0].into_iter().enumerate() {
        if round > 0 {
            decoder.reset(rate).unwrap();
        }
        let origin = 10.0 * round as f64;
        let mut blocks = Vec::new();
        for ((offset, size), time) in track.samples.iter().zip(&times) {
            let packet = &MAIN[*offset as usize..(*offset + *size) as usize];
            let pts = time.pts + (origin * f64::from(track.media_timescale)) as i64;
            while !decoder.send(packet, Some(pts)).unwrap() {
                while let Some(block) = decoder.receive().unwrap() {
                    blocks.push(block);
                }
            }
            while let Some(block) = decoder.receive().unwrap() {
                blocks.push(block);
            }
        }
        assert!(decoder.drain().unwrap());
        while let Some(block) = decoder.receive().unwrap() {
            blocks.push(block);
        }
        assert!(
            !blocks.is_empty(),
            "reset left an EOF decoder at rate {rate}"
        );
        let start = blocks[0].media_start_secs.unwrap();
        assert!(
            (start - origin - times[0].pts as f64 / f64::from(track.media_timescale)).abs() < 0.001,
            "old PCM/filter anchor survived reset: round={round}, start={start}"
        );
        assert!(blocks.iter().all(
            |block| block.samples.iter().all(|sample| sample.is_finite())
                && block.media_start_secs.unwrap() >= start
                && block.media_duration_secs > 0.0
        ));
        let frames = blocks
            .iter()
            .map(|block| block.samples.len() / usize::from(format.channels))
            .sum::<usize>();
        let media_duration = times.iter().map(|time| time.duration as f64).sum::<f64>()
            / f64::from(track.media_timescale);
        let output_duration = frames as f64 / f64::from(format.sample_rate);
        assert!(
            (output_duration * rate - media_duration).abs() < 0.15,
            "tempo reset lost tail: rate={rate}, output={output_duration}, media={media_duration}"
        );
    }
}

#[test]
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn videotoolbox_returns_native_ten_bit_surface_with_owned_lifetime() {
    use hongguo_media_types::{FrameImage, HardwareKind};
    // decode_backend drops the decoder before returning its retained frames.
    let frames = decode_backend(MAIN10, Codec::Hevc, Backend::VideoToolbox);
    let reference = decode_video(MAIN10, Codec::Hevc);
    for (frame, expected) in frames.into_iter().zip(reference) {
        // Diagnostic download exists only in this test: compare every original
        // 10-bit sample from the independent VT hardware decoder with software.
        let downloaded = frame.cpu().unwrap();
        assert_eq!(downloaded.pts, expected.pts);
        assert_eq!(downloaded.planes.len(), expected.planes.len());
        for (actual, expected) in downloaded.planes.iter().zip(&expected.planes) {
            assert_eq!(
                actual.data.as_ref(),
                expected.data.as_ref(),
                "native plane mismatch"
            );
        }
        let FrameImage::Hardware(frame) = frame.into_image().unwrap() else {
            panic!("VT candidate silently used software output");
        };
        assert_eq!(frame.handle().kind, HardwareKind::VideoToolbox);
        assert_ne!(frame.handle().object, 0);
        assert_eq!(frame.descriptor().bit_depth, 10);
    }
}

#[test]
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn videotoolbox_preserves_full_range_code_values_and_metadata() {
    const FULL: &[u8] = include_bytes!("fixtures-hevc-full-range.mp4");
    let expected = decode_video(FULL, Codec::Hevc);
    let hardware = decode_backend(FULL, Codec::Hevc, Backend::VideoToolbox);
    assert_eq!(hardware.len(), expected.len());
    for (frame, expected) in hardware.into_iter().zip(expected) {
        let actual = frame.cpu().unwrap();
        assert_eq!(actual.color.full_range, Some(true));
        assert_eq!(actual.color.full_range, expected.color.full_range);
        for (actual, expected) in actual.planes.iter().zip(&expected.planes) {
            assert_eq!(actual.data.as_ref(), expected.data.as_ref());
        }
    }
}
