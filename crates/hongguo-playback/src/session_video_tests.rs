use super::*;
use hongguo_media_types::{ChromaSampling, CropRect, NativeFrame, NativePlane};

#[test]
fn valid_container_display_metadata_overrides_bitstream_defaults_on_the_native_frame() {
    let decoded = NativeFrame {
        width: 1,
        height: 1,
        bit_depth: 8,
        chroma: ChromaSampling::Monochrome,
        planes: vec![NativePlane {
            data: Arc::from([16]),
            stride: 1,
            width: 1,
            height: 1,
            bytes_per_sample: 1,
        }],
        pts: Some(0.),
        color: ColorMetadata {
            primaries: Some(1),
            transfer: Some(1),
            matrix: Some(1),
            full_range: Some(false),
            sample_aspect_ratio: Some((1, 1)),
            ..Default::default()
        },
        crop: CropRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        rotation_degrees: 0,
    };
    let container = ColorMetadata {
        primaries: Some(9),
        transfer: Some(16),
        matrix: Some(9),
        full_range: Some(true),
        sample_aspect_ratio: Some((4, 3)),
        ..Default::default()
    };
    let result =
        metadata_image(FrameImage::Cpu(Arc::new(decoded)), None, 90, &container).descriptor();
    assert_eq!(result.color.primaries, Some(9));
    assert_eq!(result.color.transfer, Some(16));
    assert_eq!(result.color.matrix, Some(9));
    assert_eq!(result.color.full_range, Some(true));
    assert_eq!(result.color.sample_aspect_ratio, Some((4, 3)));
    assert_eq!(result.rotation_degrees, 90);
}
