//! 解码帧（紧密 I420）→ 显示端的 NV12 缓冲（带行跨距）。
//!
//! macOS 原生视频层吃的是 NV12 `CVPixelBuffer`：亮度一平面，色度 U/V 交织成第二平面，
//! 每行末尾可能有对齐填充。拷贝放在这个 crate 里而不是播放器里，是因为它逐像素跑——
//! 这个 crate 在 dev 构建档也开优化（见根 `Cargo.toml`），播放器 crate 没有。

use crate::DecodedFrame;

/// 目标 NV12 缓冲的两个平面。`luma` 至少 `luma_stride × 高` 字节，`chroma` 至少 `chroma_stride × 高/2`。
pub struct Nv12Planes<'a> {
    pub luma: &'a mut [u8],
    pub luma_stride: usize,
    pub chroma: &'a mut [u8],
    pub chroma_stride: usize,
}

/// 把一帧写进 NV12 平面。平面太小或跨距小于行宽时返回 `false`，什么都不写。
pub fn copy_to_nv12(frame: &DecodedFrame, planes: Nv12Planes<'_>) -> bool {
    let (width, height) = (frame.width, frame.height);
    let (chroma_width, chroma_height) = (width / 2, height / 2);
    let fits = planes.luma_stride >= width
        && planes.chroma_stride >= width
        && planes.luma.len() >= planes.luma_stride * height.saturating_sub(1) + width
        && planes.chroma.len() >= planes.chroma_stride * chroma_height.saturating_sub(1) + width
        && frame.y.len() >= width * height
        && frame.u.len() >= chroma_width * chroma_height
        && frame.v.len() >= chroma_width * chroma_height;
    if !fits {
        return false;
    }
    for row in 0..height {
        let to = row * planes.luma_stride;
        planes.luma[to..to + width].copy_from_slice(&frame.y[row * width..(row + 1) * width]);
    }
    for row in 0..chroma_height {
        let to = row * planes.chroma_stride;
        let line = &mut planes.chroma[to..to + chroma_width * 2];
        let u = &frame.u[row * chroma_width..(row + 1) * chroma_width];
        let v = &frame.v[row * chroma_width..(row + 1) * chroma_width];
        for ((pair, &cb), &cr) in line.as_chunks_mut::<2>().0.iter_mut().zip(u).zip(v) {
            *pair = [cb, cr];
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use hongguo_bytevc2::FramePool;

    use super::*;

    fn frame() -> DecodedFrame {
        // 4×2：亮度 0..8，U = 100、101，V = 200、201。
        FramePool::new().frame(
            4,
            2,
            [(0..8).collect(), vec![100, 101], vec![200, 201]],
            None,
        )
    }

    #[test]
    fn rows_land_on_their_strides_and_chroma_is_interleaved() {
        let mut luma = vec![0xEE; 6 * 2];
        let mut chroma = vec![0xEE; 6];
        let copied = copy_to_nv12(
            &frame(),
            Nv12Planes {
                luma: &mut luma,
                luma_stride: 6,
                chroma: &mut chroma,
                chroma_stride: 6,
            },
        );
        assert!(copied);
        assert_eq!(luma, [0, 1, 2, 3, 0xEE, 0xEE, 4, 5, 6, 7, 0xEE, 0xEE]);
        assert_eq!(
            chroma,
            [100, 200, 101, 201, 0xEE, 0xEE],
            "U/V 交织，填充不动"
        );
    }

    #[test]
    fn undersized_planes_are_refused_without_writing() {
        let mut luma = vec![0xEE; 7];
        let mut chroma = vec![0xEE; 4];
        let copied = copy_to_nv12(
            &frame(),
            Nv12Planes {
                luma: &mut luma,
                luma_stride: 4,
                chroma: &mut chroma,
                chroma_stride: 4,
            },
        );
        assert!(!copied);
        assert!(luma.iter().all(|&byte| byte == 0xEE));
    }
}
