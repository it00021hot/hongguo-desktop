#[path = "../src/output_color.rs"]
mod output_color;

use hongguo_media_types::ColorMetadata;
use hongguo_video_color::{ColorTransform, DisplayTarget};

fn metadata(transfer: u8) -> ColorMetadata {
    ColorMetadata {
        primaries: Some(if transfer == 1 { 1 } else { 9 }),
        matrix: Some(if transfer == 1 { 1 } else { 9 }),
        transfer: Some(transfer),
        full_range: Some(false),
        ..Default::default()
    }
}

fn linearize_srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn sdr_float_white_and_gray_match_the_default_unorm_relative_scale() {
    for bit_depth in [8, 10] {
        let scale = (1_u32 << (bit_depth - 8)) as f32;
        let chroma = 128. * scale;
        let baseline =
            ColorTransform::new(&metadata(1), bit_depth, 1080, DisplayTarget::sdr()).unwrap();
        let float = ColorTransform::new(
            &metadata(1),
            bit_depth,
            1080,
            output_color::target(true, 0.),
        )
        .unwrap();
        for code in [16., 32., 64., 128., 192., 235.].map(|code| code * scale) {
            let encoded = baseline.transform([code, chroma, chroma]);
            let linear = float.transform([code, chroma, chroma]);
            for (encoded, linear) in encoded.into_iter().zip(linear) {
                assert!(
                    (linearize_srgb(encoded) - linear).abs() < 0.0005,
                    "SDR code {code}: gamma={encoded} linear={linear}"
                );
            }
        }
        assert!((float.transform([235. * scale, chroma, chroma])[0] - 1.).abs() < 0.0001);
    }
}

#[test]
fn hdr_to_sdr_uses_the_same_tone_curve_for_float_and_unorm_windows() {
    for transfer in [16, 18] {
        for bit_depth in [8, 10] {
            let scale = (1_u32 << (bit_depth - 8)) as f32;
            let chroma = 128. * scale;
            let unorm = ColorTransform::new(
                &metadata(transfer),
                bit_depth,
                2160,
                output_color::target(false, 0.),
            )
            .unwrap();
            let float = ColorTransform::new(
                &metadata(transfer),
                bit_depth,
                2160,
                output_color::target(true, 0.),
            )
            .unwrap();
            for code in (16..=235).map(|code| code as f32 * scale) {
                let encoded = unorm.transform([code, chroma, chroma]);
                let linear = float.transform([code, chroma, chroma]);
                for (encoded, linear) in encoded.into_iter().zip(linear) {
                    assert!((0. ..=1.0001).contains(&linear));
                    assert!(
                        (linearize_srgb(encoded) - linear).abs() < 0.0005,
                        "HDR transfer {transfer}, code {code}: gamma={encoded} linear={linear}"
                    );
                }
            }
            let hdr = ColorTransform::new(
                &metadata(transfer),
                bit_depth,
                2160,
                output_color::target(true, 1000.),
            )
            .unwrap();
            assert!(
                hdr.transform([235. * scale, chroma, chroma])
                    .into_iter()
                    .all(|channel| channel > 1.)
            );
        }
    }
}
