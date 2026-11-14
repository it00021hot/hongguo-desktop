pub(super) fn plane_storage(
    width: usize,
    height: usize,
    stride: usize,
    bytes_per_sample: u8,
    data_length: usize,
) -> Result<(u32, u32), &'static str> {
    if width == 0 || height == 0 || !matches!(bytes_per_sample, 1 | 2) {
        return Err("invalid native plane dimensions or sample storage");
    }
    let bytes = usize::from(bytes_per_sample);
    let row = width
        .checked_mul(bytes)
        .ok_or("native plane row size overflow")?;
    if stride < row || !stride.is_multiple_of(bytes) {
        return Err("native plane stride cannot hold one complete row");
    }
    let required = (height - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(row))
        .ok_or("native plane storage overflow")?;
    if required > data_length {
        return Err("native plane is shorter than its declared rows");
    }
    u32::try_from(stride).map_err(|_| "native plane stride exceeds D3D11 limits")?;
    Ok((
        u32::try_from(width).map_err(|_| "native plane width exceeds D3D11 limits")?,
        u32::try_from(height).map_err(|_| "native plane height exceeds D3D11 limits")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_short_or_misaligned_native_planes_before_gpu_upload() {
        assert!(plane_storage(0, 1, 1, 1, 1).is_err());
        assert!(plane_storage(1, 0, 1, 1, 1).is_err());
        assert!(plane_storage(3, 2, 4, 2, 8).is_err());
        assert!(plane_storage(3, 2, 8, 2, 13).is_err());
        assert!(plane_storage(3, 2, 8, 3, 16).is_err());
    }

    #[test]
    fn preserves_odd_chroma_edges_and_non_tight_high_precision_rows() {
        assert_eq!(plane_storage(3, 2, 8, 2, 14), Ok((3, 2)));
        assert_eq!(plane_storage(3, 3, 3, 1, 9), Ok((3, 3)));
    }
}
