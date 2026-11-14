pub(crate) fn identity() -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}

pub(crate) fn expand_crop(
    mut matrix: [f32; 16],
    crop: [usize; 4],
    coded: [usize; 2],
) -> Result<[f32; 16], String> {
    let [x, y, width, height] = crop;
    if width == 0
        || height == 0
        || x.checked_add(width).is_none_or(|end| end > coded[0])
        || y.checked_add(height).is_none_or(|end| end > coded[1])
    {
        return Err("invalid external texture crop".into());
    }
    // SurfaceTexture maps [0,1] across the visible crop (and may flip Y).
    // Expand coded coordinates before applying that transform, so the shared
    // converter remains the sole owner of visible crop and rotation.
    for row in 0..4 {
        matrix[12 + row] -=
            matrix[row] * x as f32 / width as f32 + matrix[4 + row] * y as f32 / height as f32;
        matrix[row] *= coded[0] as f32 / width as f32;
        matrix[4 + row] *= coded[1] as f32 / height as f32;
    }
    Ok(matrix)
}

pub(crate) fn p010_transform(
    buffer: [usize; 2],
    image_crop: [usize; 4],
    crop: [usize; 4],
    coded: [usize; 2],
) -> Result<[f32; 16], String> {
    let [left, top, right, bottom] = image_crop;
    if buffer.contains(&0)
        || right > buffer[0]
        || bottom > buffer[1]
        || right.checked_sub(left) != Some(crop[2])
        || bottom.checked_sub(top) != Some(crop[3])
    {
        return Err("P010 image and decoded frame disagree about the visible crop".into());
    }
    let mut matrix = identity();
    matrix[0] = coded[0] as f32 / buffer[0] as f32;
    matrix[5] = coded[1] as f32 / buffer[1] as f32;
    matrix[12] = (left as f32 - crop[0] as f32) / buffer[0] as f32;
    matrix[13] = (top as f32 - crop[1] as f32) / buffer[1] as f32;
    Ok(matrix)
}
