//! YUV / NV12 工具：MediaCodec 平面打包与缩放。

use hongguo_common::{AppError, AppResult};

/// 把带 stride / slice-height 的 NV12 平面收成紧凑 NV12。
pub fn pack_nv12_from_plane(
    src: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    slice_height: usize,
) -> AppResult<Vec<u8>> {
    let need = stride * slice_height + stride * (slice_height / 2);
    if width == 0 || height == 0 {
        return Err(AppError::Media("解码输出分辨率无效".into()));
    }
    if src.len() < width * height * 3 / 2 {
        return Err(AppError::Media("解码输出缓冲过短".into()));
    }
    if stride == width && slice_height == height && src.len() >= need.min(src.len()) {
        let tight = width * height * 3 / 2;
        if src.len() >= tight && stride == width && slice_height == height {
            return Ok(src[..tight].to_vec());
        }
    }
    if src.len() < need {
        // 部分实现只按 size 给紧凑缓冲
        let tight = width * height * 3 / 2;
        if src.len() >= tight {
            return Ok(src[..tight].to_vec());
        }
        return Err(AppError::Media("解码输出缓冲与 stride 不符".into()));
    }
    let mut out = vec![0u8; width * height * 3 / 2];
    for y in 0..height {
        let s = y * stride;
        let d = y * width;
        out[d..d + width].copy_from_slice(&src[s..s + width]);
    }
    let src_uv = &src[stride * slice_height..];
    let dst_uv = &mut out[width * height..];
    for y in 0..(height / 2) {
        let s = y * stride;
        let d = y * width;
        dst_uv[d..d + width].copy_from_slice(&src_uv[s..s + width]);
    }
    Ok(out)
}

/// NV12 近邻缩放。宽高输出一律偶数。
pub fn scale_nv12(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> AppResult<Vec<u8>> {
    let (dst_w, dst_h) = (dst_w & !1, dst_h & !1);
    if dst_w < 2 || dst_h < 2 {
        return Err(AppError::Media("缩放目标过小".into()));
    }
    let need = src_w * src_h * 3 / 2;
    if src.len() < need {
        return Err(AppError::Media("NV12 源长度不足".into()));
    }
    if src_w == dst_w && src_h == dst_h {
        return Ok(src[..need].to_vec());
    }
    let mut out = vec![0u8; dst_w * dst_h * 3 / 2];
    for y in 0..dst_h {
        let sy = y * src_h / dst_h;
        for x in 0..dst_w {
            let sx = x * src_w / dst_w;
            out[y * dst_w + x] = src[sy * src_w + sx];
        }
    }
    let src_uv = &src[src_w * src_h..];
    let dst_uv = &mut out[dst_w * dst_h..];
    let (src_uv_w, src_uv_h) = (src_w / 2, src_h / 2);
    let (dst_uv_w, dst_uv_h) = (dst_w / 2, dst_h / 2);
    for y in 0..dst_uv_h {
        let sy = y * src_uv_h / dst_uv_h;
        for x in 0..dst_uv_w {
            let sx = x * src_uv_w / dst_uv_w;
            let si = (sy * src_uv_w + sx) * 2;
            let di = (y * dst_uv_w + x) * 2;
            dst_uv[di] = src_uv[si];
            dst_uv[di + 1] = src_uv[si + 1];
        }
    }
    Ok(out)
}
