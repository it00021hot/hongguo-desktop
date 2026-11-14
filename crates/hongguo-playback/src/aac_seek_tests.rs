use super::*;

#[test]
fn asc_distinguishes_lc_and_explicit_he_aac_v2_preroll() {
    let lc = AacSeek::new(&[0x12, 0x08, 0x56, 0xe5, 0], &[]).unwrap();
    assert_eq!(lc.object_type, 2);
    assert!((lc.codec_history_secs - 1024.0 / 44100.0).abs() < 1e-9);
    let he = AacSeek::new(&[0xeb, 0x8a, 0x08, 0], &[]).unwrap();
    assert_eq!(he.object_type, 29);
    assert!((he.codec_history_secs - 2048.0 / 22050.0).abs() < 1e-9);
    assert_eq!(he.bootstrap_samples, 2);
    assert!(he.history_secs(1.0, 48_000) > 1.5 && he.history_secs(1.0, 48_000) < 2.0);
    assert!(he.history_secs(3.0, 48_000) > he.history_secs(1.0, 48_000));
    assert!(AacSeek::new(&[0], &[]).is_err());
}

#[test]
fn roll_and_preroll_groups_preserve_signed_distances_and_run_boundaries() {
    use hongguo_video::r#box::build_box;
    let mut sgpd = vec![1, 0, 0, 0];
    sgpd.extend_from_slice(b"roll");
    sgpd.extend_from_slice(&2u32.to_be_bytes());
    sgpd.extend_from_slice(&2u32.to_be_bytes());
    sgpd.extend_from_slice(&(-5i16).to_be_bytes());
    sgpd.extend_from_slice(&3i16.to_be_bytes());
    let mut sbgp = vec![0, 0, 0, 0];
    sbgp.extend_from_slice(b"roll");
    sbgp.extend_from_slice(&2u32.to_be_bytes());
    for (count, index) in [(10u32, 1u32), (20, 2)] {
        sbgp.extend_from_slice(&count.to_be_bytes());
        sbgp.extend_from_slice(&index.to_be_bytes());
    }
    let mut stbl = build_box(b"sgpd", &sgpd);
    stbl.extend_from_slice(&build_box(b"sbgp", &sbgp));
    let seek = AacSeek::new(&[0x12, 0x08, 0x56, 0xe5, 0], &stbl).unwrap();
    assert_eq!(seek.group_start(8), Some(3));
    assert_eq!(seek.group_start(20), Some(17));
    // A positive recovery point remains usable beyond its group run.
    assert_eq!(seek.group_start(31), Some(28));
    let mut broken = stbl;
    broken.pop();
    assert!(AacSeek::new(&[0x12, 0x08, 0x56, 0xe5, 0], &broken).is_err());
}
