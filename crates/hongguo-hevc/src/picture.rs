use crate::ffi;
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{ChromaSampling, ColorMetadata, CropRect, NativeFrame, NativePlane};
use std::sync::Arc;

const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

pub(super) unsafe fn copy(image: *const ffi::Image, timescale: u32) -> AppResult<NativeFrame> {
    let chroma = match unsafe { ffi::de265_get_chroma_format(image) } {
        0 => ChromaSampling::Monochrome,
        1 => ChromaSampling::Yuv420,
        2 => ChromaSampling::Yuv422,
        3 => ChromaSampling::Yuv444,
        _ => return Err(AppError::Media("unsupported HEVC chroma format".into())),
    };
    let channels = if chroma == ChromaSampling::Monochrome {
        1
    } else {
        3
    };
    let depth = unsafe { ffi::de265_get_bits_per_pixel(image, 0) };
    if !(8..=16).contains(&depth) {
        return Err(AppError::Media("invalid HEVC bit depth".into()));
    }
    let mut planes = Vec::with_capacity(channels);
    let mut total = 0usize;
    for channel in 0..channels {
        let channel = channel as i32;
        let width =
            usize::try_from(unsafe { ffi::de265_get_image_width(image, channel) }).unwrap_or(0);
        let height =
            usize::try_from(unsafe { ffi::de265_get_image_height(image, channel) }).unwrap_or(0);
        let plane_depth = unsafe { ffi::de265_get_bits_per_pixel(image, channel) };
        if plane_depth != depth || width == 0 || height == 0 {
            return Err(AppError::Media(
                "invalid HEVC plane dimensions or mixed depth".into(),
            ));
        }
        let bytes_per_sample = if depth > 8 { 2 } else { 1 };
        let stride = width
            .checked_mul(bytes_per_sample)
            .ok_or_else(invalid_plane)?;
        let length = stride.checked_mul(height).ok_or_else(invalid_plane)?;
        total = total.checked_add(length).ok_or_else(invalid_plane)?;
        if total > MAX_FRAME_BYTES {
            return Err(invalid_plane());
        }
        let mut source_stride = 0;
        let pointer = unsafe { ffi::de265_get_image_plane(image, channel, &mut source_stride) };
        let source_stride = usize::try_from(source_stride).map_err(|_| invalid_plane())?;
        if pointer.is_null()
            || source_stride < stride
            || source_stride
                .checked_mul(height)
                .is_none_or(|length| length > isize::MAX as usize)
        {
            return Err(invalid_plane());
        }
        let mut data = vec![0; length];
        for (row, destination) in data.chunks_exact_mut(stride).enumerate() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    pointer.add(row * source_stride),
                    destination.as_mut_ptr(),
                    stride,
                )
            };
        }
        planes.push(NativePlane {
            data: Arc::from(data),
            stride,
            width,
            height,
            bytes_per_sample: bytes_per_sample as u8,
        });
    }
    let width = planes[0].width;
    let height = planes[0].height;
    Ok(NativeFrame {
        width,
        height,
        bit_depth: depth as u8,
        chroma,
        planes,
        pts: Some(unsafe { ffi::de265_get_image_PTS(image) } as f64 / f64::from(timescale)),
        color: ColorMetadata {
            primaries: color(unsafe { ffi::de265_get_image_colour_primaries(image) }),
            transfer: color(unsafe { ffi::de265_get_image_transfer_characteristics(image) }),
            matrix: color(unsafe { ffi::de265_get_image_matrix_coefficients(image) }),
            full_range: Some(unsafe { ffi::de265_get_image_full_range_flag(image) } != 0),
            ..Default::default()
        },
        crop: CropRect {
            width,
            height,
            ..Default::default()
        },
        rotation_degrees: 0,
    })
}

fn invalid_plane() -> AppError {
    AppError::Media("invalid or oversized HEVC image plane".into())
}

fn color(value: i32) -> Option<u8> {
    u8::try_from(value).ok().filter(|value| *value != 2)
}
