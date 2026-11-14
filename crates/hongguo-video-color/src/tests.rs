use super::*;

fn metadata(transfer: u8, matrix: u8, primaries: u8) -> ColorMetadata {
    ColorMetadata {
        transfer: Some(transfer),
        matrix: Some(matrix),
        primaries: Some(primaries),
        full_range: Some(false),
        ..Default::default()
    }
}

#[test]
fn ten_bit_video_black_and_white_follow_full_precision_range() {
    let transform =
        ColorTransform::new(&metadata(1, 1, 1), 10, 1080, DisplayTarget::sdr()).unwrap();
    let black = transform.transform([64.0, 512.0, 512.0]);
    let white = transform.transform([940.0, 512.0, 512.0]);
    assert!(black.into_iter().all(|value| value.abs() < 1e-5));
    assert!(white.into_iter().all(|value| (value - 1.0).abs() < 1e-4));
    assert_ne!(
        transform.transform([400.0, 512.0, 512.0]),
        transform.transform([401.0, 512.0, 512.0])
    );
}

#[test]
fn pq_hdr_preserves_values_above_sdr_white_in_scrgb() {
    let target = DisplayTarget {
        encoding: OutputEncoding::LinearScRgb,
        peak_nits: 10_000.0,
        ..DisplayTarget::sdr()
    };
    let transform = ColorTransform::new(&metadata(16, 9, 9), 10, 2160, target).unwrap();
    let output = transform.transform([940.0, 512.0, 512.0]);
    assert!(output.into_iter().all(|value| (value - 125.0).abs() < 0.05));
}

#[test]
fn hdr_to_sdr_has_a_monotonic_shoulder_and_keeps_shadow_detail() {
    let transform =
        ColorTransform::new(&metadata(16, 9, 9), 10, 2160, DisplayTarget::sdr()).unwrap();
    let values = (64..=940)
        .map(|code| transform.transform([code as f32, 512.0, 512.0])[0])
        .collect::<Vec<_>>();
    assert!(values.iter().all(|value| value.is_finite()));
    assert!(values.windows(2).all(|pair| pair[0] <= pair[1] + 1e-6));
    assert!(values.last().unwrap() - values[values.len() - 100] > 0.001);
    assert!(values[1] > values[0]);
    assert!((values.last().unwrap() - 1.0).abs() < 1e-4);
}

#[test]
fn hlg_ootf_uses_scene_luminance_and_reaches_reference_peak() {
    let target = DisplayTarget {
        encoding: OutputEncoding::LinearScRgb,
        peak_nits: 1000.0,
        ..DisplayTarget::sdr()
    };
    let transform = ColorTransform::new(&metadata(18, 9, 9), 10, 2160, target).unwrap();
    assert!(
        transform
            .transform([940.0, 512.0, 512.0])
            .into_iter()
            .all(|value| (value - 12.5).abs() < 0.01)
    );
}

#[test]
fn malformed_display_gamut_or_unknown_cicp_are_explicit_errors() {
    let mut target = DisplayTarget::sdr();
    target.primaries.red[1] = 0.0;
    assert!(ColorTransform::new(&metadata(16, 9, 9), 10, 2160, target).is_err());
    assert!(ColorTransform::new(&metadata(255, 9, 9), 10, 2160, DisplayTarget::sdr()).is_err());
}

#[test]
fn uniform_bytes_match_gpu_vec4_alignment() {
    let transform = ColorTransform::new(&metadata(1, 1, 1), 8, 1080, DisplayTarget::sdr()).unwrap();
    assert_eq!(transform.params.bytes().len(), 16 * 16);
    assert_eq!(transform.lut_bytes().len(), LUT_SIZE * 16);
}

#[test]
fn hlg_content_light_metadata_does_not_change_ootf_reference_or_make_black_nonfinite() {
    let mut color = metadata(18, 9, 9);
    color.content_light = Some(hongguo_media_types::ContentLightMetadata {
        max_content_light_level: 100,
        max_pic_average_light_level: 50,
    });
    let transform = ColorTransform::new(&color, 10, 2160, DisplayTarget::sdr()).unwrap();
    assert!(
        transform
            .transform([64.0, 512.0, 512.0])
            .into_iter()
            .all(|value| value.is_finite() && value.abs() < 1e-5)
    );
    assert_eq!(transform.params.source_luma[3], 1.2);
}
