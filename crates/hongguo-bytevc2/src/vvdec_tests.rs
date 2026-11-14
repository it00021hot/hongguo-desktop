use super::*;
use hongguo_media_types::ColorMetadata;

#[test]
fn native_decoder_is_pinned_to_the_verified_release() {
    assert_eq!(VvdecDecoder::version(), "3.2.1");
    assert!(VvdecDecoder::new(-2).is_err());
    let mut decoder = VvdecDecoder::new(1).unwrap();
    assert!(decoder.decode(&[], None, false).is_err());
    assert!(
        decoder
            .decode(&[0, 0, 0, 1], Some(f64::NAN), false)
            .is_err()
    );
}

#[test]
fn native_ten_bit_planes_keep_low_bits_padding_and_odd_dimensions() {
    let mut y = vec![0u8; 24];
    for row in 0..3 {
        y[row * 8 + 6..row * 8 + 8].fill(0xee);
        for col in 0..3 {
            y[row * 8 + col * 2..row * 8 + col * 2 + 2]
                .copy_from_slice(&(901u16 + row as u16 * 3 + col as u16).to_le_bytes());
        }
    }
    let mut u = vec![0x55u8; 8];
    let mut v = vec![0xaau8; 8];
    let plane = |data: &mut Vec<u8>, width, height, stride| ffi::vvdecPlane {
        ptr: data.as_mut_ptr(),
        width,
        height,
        stride,
        bytesPerSample: 2,
        allocator: ptr::null_mut(),
    };
    let raw = ffi::vvdecFrame {
        width: 3,
        height: 3,
        bitDepth: 10,
        numPlanes: 3,
        frameFormat: ffi::vvdecFrameFormat_VVDEC_FF_PROGRESSIVE,
        colorFormat: ffi::vvdecColorFormat_VVDEC_CF_YUV420_PLANAR,
        planes: [
            plane(&mut y, 3, 3, 8),
            plane(&mut u, 2, 2, 4),
            plane(&mut v, 2, 2, 4),
        ],
        ..unsafe { std::mem::zeroed() }
    };
    let frame = copy_pixels(&raw, Some(-0.125), ColorMetadata::default()).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.bit_depth, frame.pts),
        (3, 3, 10, Some(-0.125))
    );
    assert_eq!(
        frame.crop,
        CropRect {
            x: 0,
            y: 0,
            width: 3,
            height: 3
        }
    );
    assert_eq!(frame.planes[0].stride, 8);
    assert_eq!(frame.planes[0].data.len(), 24);
    assert_eq!(&frame.planes[0].data[22..24], &[0, 0]);
    assert_eq!(
        u16::from_le_bytes(frame.planes[0].data[20..22].try_into().unwrap()),
        909
    );
    assert_eq!((frame.planes[1].width, frame.planes[1].height), (2, 2));
    drop(y);
    assert_eq!(
        u16::from_le_bytes(frame.planes[0].data[..2].try_into().unwrap()),
        901
    );
}

#[test]
fn invalid_native_layouts_fail_before_dereferencing_a_plane() {
    let plane = ffi::vvdecPlane {
        ptr: ptr::null_mut(),
        width: 3,
        height: 3,
        stride: 2,
        bytesPerSample: 1,
        allocator: ptr::null_mut(),
    };
    assert!(copy_plane(&plane).is_err());
    let raw = ffi::vvdecFrame {
        width: 3,
        height: 3,
        bitDepth: 12,
        ..unsafe { std::mem::zeroed() }
    };
    assert!(copy_pixels(&raw, None, ColorMetadata::default()).is_err());
}
