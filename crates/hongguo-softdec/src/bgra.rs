//! 解码帧（紧密 I420）→ 显示端的 BGRA 缓冲（带行跨距）。
//!
//! Windows 的视频 quad 只会采样 BGRA 纹理（与 MediaEngine `TransferVideoFrame` 的产出同格式），
//! 所以软解出来的 YUV 要先在 CPU 上换成 RGB。换算按片源自己的口径（[`FrameColor`]）做，
//! 不经过「归一到 BT.601 全范围」那一步——这里没有中间方会再换一次。
//! 放在本 crate 的理由同 `nv12.rs`：逐像素循环要靠本 crate 在 dev 档也开的优化。

use crate::{DecodedFrame, FrameColor};

/// 定点小数位数（与 `color.rs` 同口径）。
const SHIFT: u32 = 14;
const ONE: f64 = (1u32 << SHIFT) as f64;
/// 四舍五入的 +0.5。
const ROUND: i32 = 1 << (SHIFT - 1);

/// 某个口径下 YUV → RGB 的定点系数。
struct Coefficients {
    luma_gain: i32,
    luma_zero: i32,
    r_from_cr: i32,
    g_from_cb: i32,
    g_from_cr: i32,
    b_from_cb: i32,
}

impl Coefficients {
    fn new(color: FrameColor) -> Self {
        let (kr, kb) = color.matrix.luma_weights();
        let kg = 1.0 - kr - kb;
        let r_pr = 2.0 * (1.0 - kr);
        let b_pb = 2.0 * (1.0 - kb);
        let g_pb = -kb * b_pb / kg;
        let g_pr = -kr * r_pr / kg;
        let (luma_span, chroma_span, luma_zero) = if color.full_range {
            (255.0, 255.0, 0)
        } else {
            (219.0, 224.0, 16)
        };
        let chroma_gain = 255.0 / chroma_span;
        let fixed = |value: f64| (value * ONE).round() as i32;
        Self {
            luma_gain: fixed(255.0 / luma_span),
            luma_zero,
            r_from_cr: fixed(r_pr * chroma_gain),
            g_from_cb: fixed(g_pb * chroma_gain),
            g_from_cr: fixed(g_pr * chroma_gain),
            b_from_cb: fixed(b_pb * chroma_gain),
        }
    }
}

fn clamp8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// BGRA 输出至少要多少字节。
pub fn bgra_len(width: usize, height: usize, stride: usize) -> usize {
    if height == 0 {
        0
    } else {
        stride * (height - 1) + width * 4
    }
}

/// 按 `color` 把一帧换成 BGRA（alpha 恒 255）写进 `out`。缓冲太小、跨距不足一行或平面不全时
/// 返回 `false`，什么都不写。奇数宽高时最后一列/行沿用最近的色度点（与 I420 的取整约定一致）。
pub fn copy_to_bgra(
    frame: &DecodedFrame,
    color: FrameColor,
    out: &mut [u8],
    stride: usize,
) -> bool {
    let (width, height) = (frame.width, frame.height);
    let (chroma_width, chroma_height) = (width / 2, height / 2);
    let fits = width > 0
        && height > 0
        && chroma_width > 0
        && chroma_height > 0
        && stride >= width * 4
        && out.len() >= bgra_len(width, height, stride)
        && frame.y.len() >= width * height
        && frame.u.len() >= chroma_width * chroma_height
        && frame.v.len() >= chroma_width * chroma_height;
    if !fits {
        return false;
    }
    let table = Coefficients::new(color);
    // 每个亮度列一组色度贡献（一个色度点管两列），亮度那一遍就是几个等长切片逐元素对着算。
    let mut red = vec![0i32; width];
    let mut green = vec![0i32; width];
    let mut blue = vec![0i32; width];
    for chroma_row in 0..height.div_ceil(2) {
        let chroma_at = chroma_row.min(chroma_height - 1) * chroma_width;
        let u_row = &frame.u[chroma_at..chroma_at + chroma_width];
        let v_row = &frame.v[chroma_at..chroma_at + chroma_width];
        for (column, ((r, g), b)) in red
            .iter_mut()
            .zip(green.iter_mut())
            .zip(blue.iter_mut())
            .enumerate()
        {
            let at = (column / 2).min(chroma_width - 1);
            let cb = i32::from(u_row[at]) - 128;
            let cr = i32::from(v_row[at]) - 128;
            *r = table.r_from_cr.wrapping_mul(cr).wrapping_add(ROUND);
            *g = table
                .g_from_cb
                .wrapping_mul(cb)
                .wrapping_add(table.g_from_cr.wrapping_mul(cr))
                .wrapping_add(ROUND);
            *b = table.b_from_cb.wrapping_mul(cb).wrapping_add(ROUND);
        }
        for luma_row in (chroma_row * 2..chroma_row * 2 + 2).filter(|&row| row < height) {
            let luma = &frame.y[luma_row * width..(luma_row + 1) * width];
            let line = &mut out[luma_row * stride..luma_row * stride + width * 4];
            let pixels = line.as_chunks_mut::<4>().0.iter_mut();
            for ((((pixel, &y), &r), &g), &b) in pixels.zip(luma).zip(&red).zip(&green).zip(&blue) {
                // 环绕运算的理由同 `color.rs` 的 `mul_add`：数值上溢不了，换来 dev 档也能向量化。
                let scaled = table
                    .luma_gain
                    .wrapping_mul(i32::from(y).wrapping_sub(table.luma_zero));
                *pixel = [
                    clamp8(scaled.wrapping_add(b) >> SHIFT),
                    clamp8(scaled.wrapping_add(g) >> SHIFT),
                    clamp8(scaled.wrapping_add(r) >> SHIFT),
                    255,
                ];
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use hongguo_bytevc2::FramePool;

    use super::*;
    use crate::Matrix;

    fn frame(width: usize, height: usize, yuv: (u8, u8, u8)) -> DecodedFrame {
        let chroma = width / 2 * (height / 2);
        FramePool::new().frame(
            width,
            height,
            [
                vec![yuv.0; width * height],
                vec![yuv.1; chroma],
                vec![yuv.2; chroma],
            ],
            Some(0.0),
        )
    }

    fn shown(color: FrameColor, yuv: (u8, u8, u8)) -> [i32; 3] {
        let (width, height) = (4, 4);
        let stride = width * 4 + 8;
        let mut out = vec![0u8; bgra_len(width, height, stride)];
        assert!(copy_to_bgra(
            &frame(width, height, yuv),
            color,
            &mut out,
            stride
        ));
        let first = [out[0], out[1], out[2], out[3]];
        for row in 0..height {
            for pixel in out[row * stride..row * stride + width * 4]
                .as_chunks::<4>()
                .0
            {
                assert_eq!(*pixel, first, "整帧同色，换完也应整帧同色");
            }
        }
        assert_eq!(first[3], 255, "alpha 恒不透明");
        [
            i32::from(first[2]),
            i32::from(first[1]),
            i32::from(first[0]),
        ]
    }

    fn assert_rgb(got: [i32; 3], want: [i32; 3], what: &str) {
        for (got, want) in got.iter().zip(want) {
            assert!(
                (got - want).abs() <= 2,
                "{what}: 得到 {got:?}，应为 {want:?}"
            );
        }
    }

    const BT709_LIMITED: FrameColor = FrameColor {
        matrix: Matrix::Bt709,
        full_range: false,
    };

    #[test]
    fn bt709_limited_reference_colours() {
        assert_rgb(shown(BT709_LIMITED, (16, 128, 128)), [0, 0, 0], "黑");
        assert_rgb(shown(BT709_LIMITED, (235, 128, 128)), [255, 255, 255], "白");
        assert_rgb(shown(BT709_LIMITED, (63, 102, 240)), [255, 0, 0], "红");
        assert_rgb(shown(BT709_LIMITED, (173, 42, 26)), [0, 255, 0], "绿");
        assert_rgb(shown(BT709_LIMITED, (32, 240, 118)), [0, 0, 255], "蓝");
    }

    #[test]
    fn bt601_full_matches_the_jpeg_formula() {
        let yuv = (90, 200, 60);
        let (y, cb, cr) = (90.0, 200.0 - 128.0, 60.0 - 128.0);
        let want = [
            y + 1.402 * cr,
            y - 0.344_136 * cb - 0.714_136 * cr,
            y + 1.772 * cb,
        ]
        .map(|channel: f64| channel.round().clamp(0.0, 255.0) as i32);
        assert_rgb(shown(FrameColor::BT601_FULL, yuv), want, "BT.601 全范围");
    }

    #[test]
    fn each_chroma_sample_colours_its_own_2x2_block_and_odd_edges_reuse_the_last() {
        // 5×3：两列色度点、一行色度点；第 5 列与第 3 行没有自己的色度点，沿用最近那个。
        let mut frame = frame(5, 3, (120, 128, 128));
        frame.u[1] = 220;
        let stride = 5 * 4;
        let mut out = vec![0u8; bgra_len(5, 3, stride)];
        assert!(copy_to_bgra(&frame, BT709_LIMITED, &mut out, stride));
        let pixel = |x: usize, y: usize| &out[y * stride + x * 4..y * stride + x * 4 + 4];
        assert_eq!(pixel(0, 0), pixel(1, 1));
        assert_eq!(pixel(2, 0), pixel(3, 1));
        assert_ne!(pixel(0, 0), pixel(2, 0), "带色的那块应跟中性色不同");
        assert_eq!(pixel(4, 0), pixel(3, 0), "奇数宽的最后一列沿用左边的色度点");
        assert_eq!(pixel(0, 2), pixel(0, 1), "奇数高的最后一行沿用上面的色度点");
    }

    #[test]
    fn refuses_buffers_that_do_not_fit() {
        let frame = frame(4, 4, (16, 128, 128));
        let mut small = vec![0u8; bgra_len(4, 4, 16) - 1];
        assert!(!copy_to_bgra(&frame, BT709_LIMITED, &mut small, 16));
        let mut narrow = vec![0u8; 64];
        assert!(!copy_to_bgra(&frame, BT709_LIMITED, &mut narrow, 15));
    }
}
