//! 解码帧的 YUV 口径（用哪套矩阵、是不是全范围），以及把它归一到「BT.601 全范围」。
//!
//! 为什么要归一：WebView 把 I420 帧画上画布时，换算口径不归我们管。实测 WKWebView 对
//! `VideoFrame` 一律按 BT.601 全范围换算，传给它的 `colorSpace` 完全不看（2026-11-11 用已知色值
//! 标定）。高清片源是 BT.709 限幅范围，原样送过去画面就发灰、色相偏。所以在出帧这一侧把 YUV
//! 换成 BT.601 全范围，再如实声明——守规矩的浏览器按声明换算，不守规矩的那个恰好也是这套。
//!
//! 换算只动矩阵和范围。色域（primaries）与传递函数不处理，HDR 片源不在支持范围内。

use crate::DecodedFrame;

/// YUV ↔ RGB 的矩阵系数族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    /// (Kr, Kb)。Kg = 1 − Kr − Kb。
    pub(crate) fn luma_weights(self) -> (f64, f64) {
        match self {
            Matrix::Bt601 => (0.299, 0.114),
            Matrix::Bt709 => (0.2126, 0.0722),
            Matrix::Bt2020 => (0.2627, 0.0593),
        }
    }
}

/// 一条视频轨的 YUV 口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameColor {
    pub matrix: Matrix,
    /// 全范围（0–255）还是限幅范围（亮度 16–235、色度 16–240）。
    pub full_range: bool,
}

impl FrameColor {
    /// 归一的目标口径。
    pub const BT601_FULL: Self = Self {
        matrix: Matrix::Bt601,
        full_range: true,
    };

    /// 码流没写矩阵时的惯例：高清按 BT.709，标清按 BT.601；范围另算。
    /// 这是业界通行的猜法，不是从码流里读到的事实。
    pub fn assumed(height: usize, full_range: bool) -> Self {
        Self {
            matrix: if height >= 720 {
                Matrix::Bt709
            } else {
                Matrix::Bt601
            },
            full_range,
        }
    }

    /// 按 H.273 的 `matrix_coeffs` 取口径；没写（2）、写了本项目不处理的值时退回 [`Self::assumed`]。
    /// 0 是 RGB 恒等矩阵，4:2:0 视频里出现它只可能是「没写、读成了缺省 0」，同样按没写处理。
    pub fn from_matrix_coeffs(matrix_coeffs: u8, full_range: bool, height: usize) -> Self {
        let matrix = match matrix_coeffs {
            1 => Matrix::Bt709,
            5 | 6 => Matrix::Bt601,
            9 | 10 => Matrix::Bt2020,
            _ => return Self::assumed(height, full_range),
        };
        Self { matrix, full_range }
    }

    /// 把一帧从本口径就地换成 [`Self::BT601_FULL`]。已经是目标口径时什么都不做。
    pub fn convert_to_bt601_full(self, frame: &mut DecodedFrame) {
        if self == Self::BT601_FULL {
            return;
        }
        let table = Conversion::new(self);
        let (width, height) = (frame.width, frame.height);
        let chroma_width = width / 2;
        // 每个亮度列一个偏移（一个色度点管两列），这样亮度那一遍就是两个等长切片逐元素对着算。
        let mut luma_shift = vec![0i32; chroma_width * 2];
        for chroma_row in 0..height / 2 {
            let chroma_at = chroma_row * chroma_width;
            let u_row = &mut frame.u[chroma_at..chroma_at + chroma_width];
            let v_row = &mut frame.v[chroma_at..chroma_at + chroma_width];
            // 亮度的新值要用到原来的色度，所以先算出色度给亮度带来的偏移，再改色度本身。
            let pairs = luma_shift.as_chunks_mut::<2>().0.iter_mut();
            for ((pair, &u), &v) in pairs.zip(&*u_row).zip(&*v_row) {
                let (cb, cr) = centred(u, v);
                *pair = [mul_add(table.y_from_cb, cb, table.y_from_cr, cr, table.y_bias); 2];
            }
            for (u, v) in u_row.iter_mut().zip(v_row.iter_mut()) {
                let (cb, cr) = centred(*u, *v);
                *u =
                    clamp8(mul_add(table.u_from_cb, cb, table.u_from_cr, cr, CHROMA_BIAS) >> SHIFT);
                *v =
                    clamp8(mul_add(table.v_from_cb, cb, table.v_from_cr, cr, CHROMA_BIAS) >> SHIFT);
            }
            // 一个色度点管 2×2 个亮度点：上下两行用同一组偏移。
            for luma_row in [chroma_row * 2, chroma_row * 2 + 1] {
                let row = &mut frame.y[luma_row * width..luma_row * width + chroma_width * 2];
                for (y, &shift) in row.iter_mut().zip(&luma_shift) {
                    *y = clamp8(
                        table
                            .y_from_y
                            .wrapping_mul(i32::from(*y))
                            .wrapping_add(shift)
                            >> SHIFT,
                    );
                }
            }
        }
    }
}

/// 色度样本去掉 128 的中心。
fn centred(u: u8, v: u8) -> (i32, i32) {
    (i32::from(u) - 128, i32::from(v) - 128)
}

/// `a·x + b·y + c`。
///
/// 用环绕运算而不是普通的 `*` `+`：数值上溢不了（系数绝对值不到 2¹⁵，样本不到 2⁸，三项相加离
/// `i32` 上限差着几十倍），但 dev 档默认开整数溢出检查，带着检查这几个逐像素循环就没法向量化——
/// 实测 dev 构建里换算因此吃掉解码线程两成时间。写成环绕运算，快慢就不取决于构建档了。
fn mul_add(a: i32, x: i32, b: i32, y: i32, c: i32) -> i32 {
    a.wrapping_mul(x)
        .wrapping_add(b.wrapping_mul(y))
        .wrapping_add(c)
}

/// 定点小数位数。系数绝对值不超过 2，输入不超过 255，乘加之后离 `i32` 上限还很远。
const SHIFT: u32 = 14;
const ONE: f64 = (1u32 << SHIFT) as f64;
/// 色度输出的 +128 与四舍五入的 +0.5，合在一个定点常数里。
const CHROMA_BIAS: i32 = (128 << SHIFT) + (1 << (SHIFT - 1));

fn clamp8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// 源口径 → BT.601 全范围的定点系数。
///
/// 推导：先把源 YUV 归一成 `Y ∈ [0,1]`、`Pb/Pr ∈ [-0.5,0.5]`，按源矩阵得 RGB，再按 BT.601
/// 得目标的 `Y'/Pb'/Pr'`。三个亮度权重加起来是 1，所以目标亮度 = 源亮度 + 色度的线性组合，
/// 目标色度只和源色度有关——这正好对上 4:2:0 的结构：色度在半分辨率上自己换，亮度逐点加一个偏移。
struct Conversion {
    y_from_y: i32,
    y_from_cb: i32,
    y_from_cr: i32,
    /// 亮度的常数项：源亮度的零点（限幅范围的 16）与四舍五入。
    y_bias: i32,
    u_from_cb: i32,
    u_from_cr: i32,
    v_from_cb: i32,
    v_from_cr: i32,
}

impl Conversion {
    fn new(source: FrameColor) -> Self {
        let (kr, kb) = source.matrix.luma_weights();
        let kg = 1.0 - kr - kb;
        let (tr, tb) = Matrix::Bt601.luma_weights();
        let tg = 1.0 - tr - tb;
        // 源：R = Y + r_pr·Pr，B = Y + b_pb·Pb，G = Y + g_pb·Pb + g_pr·Pr。
        let r_pr = 2.0 * (1.0 - kr);
        let b_pb = 2.0 * (1.0 - kb);
        let g_pb = -kb * b_pb / kg;
        let g_pr = -kr * r_pr / kg;
        // 目标亮度 = Y + y_pb·Pb + y_pr·Pr。
        let y_pb = tg * g_pb + tb * b_pb;
        let y_pr = tr * r_pr + tg * g_pr;
        // 目标色度：Pb' = (B − Y') / (2(1 − tb))，Pr' = (R − Y') / (2(1 − tr))。
        let pb_pb = (b_pb - y_pb) / (2.0 * (1.0 - tb));
        let pb_pr = -y_pr / (2.0 * (1.0 - tb));
        let pr_pb = -y_pb / (2.0 * (1.0 - tr));
        let pr_pr = (r_pr - y_pr) / (2.0 * (1.0 - tr));
        // 源样本到归一值的刻度：限幅范围亮度跨 219、色度跨 224，全范围都是 255。目标刻度是 255。
        let (luma_span, chroma_span, luma_zero) = if source.full_range {
            (255.0, 255.0, 0.0)
        } else {
            (219.0, 224.0, 16.0)
        };
        let luma_gain = 255.0 / luma_span;
        let chroma_gain = 255.0 / chroma_span;
        let fixed = |value: f64| (value * ONE).round() as i32;
        Self {
            y_from_y: fixed(luma_gain),
            y_from_cb: fixed(y_pb * chroma_gain),
            y_from_cr: fixed(y_pr * chroma_gain),
            y_bias: fixed(-luma_zero * luma_gain) + (1 << (SHIFT - 1)),
            u_from_cb: fixed(pb_pb * chroma_gain),
            u_from_cr: fixed(pb_pr * chroma_gain),
            v_from_cb: fixed(pr_pb * chroma_gain),
            v_from_cr: fixed(pr_pr * chroma_gain),
        }
    }
}

#[cfg(test)]
mod tests {
    use hongguo_bytevc2::FramePool;

    use super::*;

    fn frame(width: usize, height: usize, yuv: (u8, u8, u8)) -> DecodedFrame {
        let chroma = width / 2 * height / 2;
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

    /// 按 BT.601 全范围把一个 YUV 点换成 RGB——WKWebView 实际用的就是这套。
    fn bt601_full_rgb(y: u8, u: u8, v: u8) -> [i32; 3] {
        let (y, cb, cr) = (f64::from(y), f64::from(u) - 128.0, f64::from(v) - 128.0);
        [
            y + 1.402 * cr,
            y - 0.344_136 * cb - 0.714_136 * cr,
            y + 1.772 * cb,
        ]
        .map(|channel| channel.round().clamp(0.0, 255.0) as i32)
    }

    fn shown(source: FrameColor, yuv: (u8, u8, u8)) -> [i32; 3] {
        let mut frame = frame(4, 4, yuv);
        source.convert_to_bt601_full(&mut frame);
        // 整帧同色，换完也应整帧同色。
        assert!(frame.y.iter().all(|&y| y == frame.y[0]));
        assert!(frame.u.iter().all(|&u| u == frame.u[0]));
        bt601_full_rgb(frame.y[0], frame.u[0], frame.v[0])
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
    fn bt709_limited_black_white_and_grey_reach_the_full_range() {
        assert_rgb(shown(BT709_LIMITED, (16, 128, 128)), [0, 0, 0], "黑");
        assert_rgb(shown(BT709_LIMITED, (235, 128, 128)), [255, 255, 255], "白");
        assert_rgb(
            shown(BT709_LIMITED, (126, 128, 128)),
            [128, 128, 128],
            "中灰",
        );
    }

    #[test]
    fn bt709_limited_primaries_come_out_pure() {
        // BT.709 限幅范围下纯红、纯绿、纯蓝的标准码值。
        assert_rgb(shown(BT709_LIMITED, (63, 102, 240)), [255, 0, 0], "红");
        assert_rgb(shown(BT709_LIMITED, (173, 42, 26)), [0, 255, 0], "绿");
        assert_rgb(shown(BT709_LIMITED, (32, 240, 118)), [0, 0, 255], "蓝");
    }

    #[test]
    fn bt601_limited_only_needs_its_range_stretched() {
        let source = FrameColor {
            matrix: Matrix::Bt601,
            full_range: false,
        };
        assert_rgb(shown(source, (16, 128, 128)), [0, 0, 0], "黑");
        assert_rgb(shown(source, (81, 90, 240)), [255, 0, 0], "红");
        assert_rgb(shown(source, (41, 240, 110)), [0, 0, 255], "蓝");
    }

    #[test]
    fn bt709_full_range_keeps_its_levels_and_only_changes_matrix() {
        let source = FrameColor {
            matrix: Matrix::Bt709,
            full_range: true,
        };
        assert_rgb(shown(source, (0, 128, 128)), [0, 0, 0], "黑");
        assert_rgb(shown(source, (255, 128, 128)), [255, 255, 255], "白");
        assert_rgb(shown(source, (54, 99, 255)), [255, 0, 0], "红");
    }

    #[test]
    fn the_target_colour_is_left_untouched() {
        let mut frame = frame(4, 4, (37, 201, 90));
        FrameColor::BT601_FULL.convert_to_bt601_full(&mut frame);
        assert_eq!((frame.y[0], frame.u[0], frame.v[0]), (37, 201, 90));
    }

    #[test]
    fn each_chroma_sample_shifts_exactly_its_own_2x2_luma_block() {
        // 4×2 的帧有两个色度点：左边是中性色，右边带色。亮度全相同，换完左右两块应各自一致、彼此不同。
        let mut frame = frame(4, 2, (100, 128, 128));
        frame.u[1] = 200;
        frame.v[1] = 60;
        BT709_LIMITED.convert_to_bt601_full(&mut frame);
        let (left, right) = (frame.y[0], frame.y[2]);
        assert_eq!([frame.y[0], frame.y[1], frame.y[4], frame.y[5]], [left; 4]);
        assert_eq!([frame.y[2], frame.y[3], frame.y[6], frame.y[7]], [right; 4]);
        assert_ne!(left, right, "带色的那块亮度应跟着色度变");
    }

    #[test]
    fn unspecified_matrix_follows_the_resolution_convention() {
        assert_eq!(
            FrameColor::from_matrix_coeffs(2, false, 1080).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(0, false, 720).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(2, false, 480).matrix,
            Matrix::Bt601
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(1, false, 480).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(6, true, 1080),
            FrameColor::BT601_FULL
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(9, false, 2160).matrix,
            Matrix::Bt2020
        );
    }
}
