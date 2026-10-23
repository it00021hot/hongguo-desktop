//! 4 字节像素的通道序：RGBA ⇄ BGRA（就地把 R 与 B 对调）。
//!
//! 给**渲染上传端**用：gpui 的 `RenderImage` 约定帧缓冲是 BGRA（见
//! `hongguo-gpui/src/image_upload.rs` 的模块头），而 `image` crate 解出来的
//! 位图是 RGBA——上传前必须过一遍这里，否则画面上 R 与 B 整体互换：
//! 蓝底海报变橙底、正常肤色变蓝脸。
//!
//! 纯函数、零依赖。行为由本模块单测锁死——渲染路径没有视觉测试，单测是唯一
//! 能把这条不变量自动化盯住的地方。

/// 就地交换每个 4 字节像素的 R 与 B（RGBA → BGRA；自反：再来一次回到 RGBA）。
///
/// 缓冲长度不是 4 的倍数时，末尾不足一像素的字节原样保留（不猜测对齐）。
pub fn swap_red_blue_in_place(bytes: &mut [u8]) {
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只动 R 与 B：G、A 原位不动（这正是「蓝底变橙底、绿植照旧」的成因）。
    #[test]
    fn swaps_only_red_and_blue() {
        let mut pixel = [10u8, 20, 30, 40];
        swap_red_blue_in_place(&mut pixel);
        assert_eq!(pixel, [30, 20, 10, 40]);
    }

    /// 自反：交换两次回到原状（RGBA → BGRA 与 BGRA → RGBA 是同一个操作）。
    #[test]
    fn is_self_inverse() {
        let mut bytes: Vec<u8> = (0..4 * 7).map(|i| i as u8).collect();
        let original = bytes.clone();
        swap_red_blue_in_place(&mut bytes);
        assert_ne!(bytes, original);
        swap_red_blue_in_place(&mut bytes);
        assert_eq!(bytes, original);
    }

    /// 逐像素、逐通道对位：长度不变，每个像素的 R/B 各自对调，A 保留。
    #[test]
    fn swaps_each_pixel_independently() {
        let mut bytes = vec![1u8, 2, 3, 200, 4, 5, 6, 255];
        swap_red_blue_in_place(&mut bytes);
        assert_eq!(bytes, vec![3, 2, 1, 200, 6, 5, 4, 255]);
    }

    /// 尾巴不足一像素的字节不碰（不猜对齐，保留原样）。
    #[test]
    fn tail_shorter_than_a_pixel_is_left_alone() {
        let mut bytes = vec![1u8, 2, 3, 4, 9, 9, 9];
        swap_red_blue_in_place(&mut bytes);
        assert_eq!(bytes, vec![3, 2, 1, 4, 9, 9, 9]);
    }

    /// 中性像素（R=G=B）交换前后一模一样——所以 R/B 互换在灰阶画面上看不出来，
    /// 只在有色内容上现形（蓝↔橙、肤色翻转）。
    #[test]
    fn neutral_pixels_are_unchanged() {
        let mut bytes = vec![128u8, 128, 128, 255, 0, 0, 0, 255];
        swap_red_blue_in_place(&mut bytes);
        assert_eq!(bytes, vec![128, 128, 128, 255, 0, 0, 0, 255]);
    }
}
