#[path = "../src/surface_geometry.rs"]
mod geometry;

#[test]
fn texture_crop_is_inverted_before_the_shared_crop_is_applied() {
    // SurfaceTexture's transform maps the visible rectangle to an already
    // cropped image. The native frame uses coded coordinates, including odd crop.
    let mut matrix = geometry::identity();
    matrix[0] = 0.75;
    matrix[5] = -0.5;
    matrix[12] = 0.125;
    matrix[13] = 0.75;
    let expanded = geometry::expand_crop(matrix, [1, 1, 6, 4], [8, 8]).unwrap();
    assert_eq!(expanded[0], 1.0);
    assert_eq!(expanded[5], -1.0);
    assert_eq!(expanded[12], 0.0);
    assert_eq!(expanded[13], 0.875);
}

#[test]
fn p010_buffer_padding_and_crop_have_one_explicit_mapping() {
    let matrix = geometry::p010_transform(
        [1920, 1088],
        [0, 0, 1920, 1080],
        [0, 0, 1920, 1080],
        [1920, 1080],
    )
    .unwrap();
    assert_eq!(matrix[0], 1.0);
    assert!((matrix[5] - 1080.0 / 1088.0).abs() < 1e-6);
    assert!(geometry::p010_transform([10, 10], [1, 1, 9, 9], [0, 0, 9, 9], [9, 9]).is_err());
    assert!(geometry::expand_crop(geometry::identity(), [0, 0, 0, 9], [9, 9]).is_err());
}
