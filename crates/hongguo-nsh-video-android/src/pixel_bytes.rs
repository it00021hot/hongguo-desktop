pub(crate) fn swizzle(
    width: u32,
    height: u32,
    stride: u32,
    pixels: &[u8],
    rgba: &mut Vec<u8>,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("empty direct image".into());
    }
    let row = usize::try_from(u64::from(width) * 4).map_err(|_| "direct row overflow")?;
    let length = row
        .checked_mul(height as usize)
        .ok_or("direct image overflow")?;
    let stride = stride as usize;
    if stride < row
        || stride
            .checked_mul(height as usize)
            .is_none_or(|length| length > pixels.len())
    {
        return Err("invalid direct BGRA storage".into());
    }
    rgba.resize(length, 0);
    for (output, input) in rgba.chunks_exact_mut(row).zip(pixels.chunks_exact(stride)) {
        output.copy_from_slice(&input[..row]);
        for pixel in output.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Ok(())
}
