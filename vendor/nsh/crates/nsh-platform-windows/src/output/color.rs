pub(super) const REFERENCE_WHITE_NITS: f32 = 80.0;

pub(super) fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub(super) fn ui_color(value: [f32; 4], white_nits: f32) -> [f32; 4] {
    let scale = white_nits / REFERENCE_WHITE_NITS;
    [
        srgb_to_linear(value[0]) * scale,
        srgb_to_linear(value[1]) * scale,
        srgb_to_linear(value[2]) * scale,
        value[3],
    ]
}

pub(super) fn bgra_to_scrgb(
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    white_nits: f32,
) -> Option<Vec<u16>> {
    if width == 0 || height == 0 || stride == 0 {
        return None;
    }
    let row_bytes = usize::try_from(width).ok()?.checked_mul(4)?;
    let stride = usize::try_from(stride).ok()?;
    let height = usize::try_from(height).ok()?;
    if stride < row_bytes || pixels.len() < stride.checked_mul(height)? {
        return None;
    }
    let mut output = Vec::with_capacity(row_bytes.checked_mul(height)?);
    let scale = white_nits / REFERENCE_WHITE_NITS;
    for row in pixels.chunks(stride).take(height) {
        for pixel in row[..row_bytes].as_chunks::<4>().0 {
            let alpha = f32::from(pixel[3]) / 255.0;
            for channel in [pixel[2], pixel[1], pixel[0]] {
                let linear = if alpha == 0.0 {
                    0.0
                } else {
                    srgb_to_linear((f32::from(channel) / 255.0 / alpha).min(1.0)) * alpha * scale
                };
                output.push(half_bits(linear));
            }
            output.push(half_bits(alpha));
        }
    }
    Some(output)
}

fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let fraction = bits & 0x007f_ffff;
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let significand = fraction | 0x0080_0000;
        let shift = (14 - exponent) as u32;
        let rounding = (1 << (shift - 1)) - 1 + ((significand >> shift) & 1);
        return sign | ((significand + rounding) >> shift) as u16;
    }
    if exponent >= 31 {
        return sign | 0x7c00 | if value.is_nan() { 0x0200 } else { 0 };
    }
    let rounded = fraction + 0x0fff + ((fraction >> 13) & 1);
    sign | (((exponent as u32) << 10) + (rounded >> 13)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_srgb_midgray_is_linearized_before_scrgb_composition() {
        let result = ui_color([0.5, 0.5, 0.5, 0.4], REFERENCE_WHITE_NITS);
        assert!((result[0] - 0.21404114).abs() < 0.000001);
        assert_eq!(result[3], 0.4);
    }

    #[test]
    fn os_sdr_white_scales_ui_without_clipping_or_scaling_alpha() {
        assert_eq!(ui_color([1.0, 1.0, 1.0, 0.5], 200.0), [2.5, 2.5, 2.5, 0.5]);
    }

    #[test]
    fn sdr_bitmap_upload_has_rgba_order_and_preserves_hdr_white() {
        let output = bgra_to_scrgb(&[0, 0, 255, 255, 255, 255, 255, 255], 2, 1, 8, 200.0);
        assert_eq!(
            output,
            Some(vec![0x4100, 0, 0, 0x3c00, 0x4100, 0x4100, 0x4100, 0x3c00])
        );
    }

    #[test]
    fn premultiplied_sdr_is_unpremultiplied_before_transfer_conversion() {
        let output = bgra_to_scrgb(&[0, 0, 64, 128], 1, 1, 4, REFERENCE_WHITE_NITS).unwrap();
        let expected = half_bits(srgb_to_linear(0.5) * (128.0 / 255.0));
        assert_eq!(output, [expected, 0, 0, half_bits(128.0 / 255.0)]);
        assert_eq!(
            bgra_to_scrgb(&[255, 255, 255, 0], 1, 1, 4, 200.0),
            Some(vec![0; 4])
        );
    }

    #[test]
    fn bitmap_rows_respect_stride_and_reject_short_buffers() {
        assert_eq!(bgra_to_scrgb(&[], 0, 0, 0, 80.0), None);
        assert_eq!(bgra_to_scrgb(&[0; 3], 1, 1, 4, 80.0), None);
        assert_eq!(bgra_to_scrgb(&[0; 8], 2, 1, 4, 80.0), None);
        assert_eq!(bgra_to_scrgb(&[0; 16], 1, 2, 8, 80.0), Some(vec![0; 8]));
    }

    #[test]
    fn half_rounding_keeps_subnormal_alpha_products_and_highlights() {
        assert_eq!(half_bits(0.0), 0);
        assert_eq!(half_bits(1.0), 0x3c00);
        assert_eq!(half_bits(2.5), 0x4100);
        assert_eq!(half_bits(2.0_f32.powi(-24)), 1);
        assert_eq!(half_bits(2.0_f32.powi(-25)), 0);
        assert_eq!(half_bits(3.0 * 2.0_f32.powi(-25)), 2);
    }
}
