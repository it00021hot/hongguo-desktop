#[path = "../src/pixel_bytes.rs"]
mod pixel_bytes;

#[test]
fn bgra_upload_preserves_alpha_and_skips_stride_padding() {
    let mut rgba = Vec::new();
    pixel_bytes::swizzle(
        1,
        2,
        8,
        &[1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8, 0, 0, 0, 0],
        &mut rgba,
    )
    .unwrap();
    assert_eq!(rgba, [3, 2, 1, 4, 7, 6, 5, 8]);
    pixel_bytes::swizzle(1, 1, 4, &[9, 10, 11, 12], &mut rgba).unwrap();
    assert_eq!(rgba, [11, 10, 9, 12]);
}

#[test]
fn malformed_frames_fail_before_touching_upload_storage() {
    for (width, height, stride, bytes) in [
        (0, 1, 0, 0),
        (1, 0, 4, 4),
        (2, 1, 4, 8),
        (1, 2, 8, 12),
        (u32::MAX, u32::MAX, u32::MAX, 0),
    ] {
        let mut rgba = vec![42];
        assert!(pixel_bytes::swizzle(width, height, stride, &vec![0; bytes], &mut rgba).is_err());
        assert_eq!(rgba, [42]);
    }
}
