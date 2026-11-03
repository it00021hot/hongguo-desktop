//! `image` crate 的 RGBA 位图 → gpui [`RenderImage`] 的唯一入口（必须在此
//! 转 BGRA：`RenderImage` 约定帧缓冲是 BGRA，`RenderImage::new` 自身不做
//! 转换）。本壳手工造帧的地方都走这一个函数，别在调用点各自交换；
//! 「只有这里能造 `RenderImage`」由 `tests/render_image_guard.rs` 扫源码盯着。

use gpui_kit::RenderImage;

/// RGBA 位图 → 可上屏的 `RenderImage`（就地交换成 BGRA）。
pub(crate) fn render_image(mut rgba: image::RgbaImage) -> RenderImage {
    let bytes: &mut [u8] = rgba.as_mut();
    hongguo_media::bgra::swap_red_blue_in_place(bytes);
    RenderImage::new(smallvec::smallvec![image::Frame::new(rgba)])
}

/// 多帧位图（GIF 解出的动图帧序列，各帧带 delay）→ 可上屏的 `RenderImage`。
/// `img` 元素对多帧图按帧 delay 自动推进（gpui-pre `elements/img.rs`），
/// BGRA 交换纪律与单帧版一致。
pub(crate) fn render_image_frames(frames: Vec<image::Frame>) -> RenderImage {
    let mut frames = frames;
    for frame in &mut frames {
        let bytes: &mut [u8] = frame.buffer_mut().as_mut();
        hongguo_media::bgra::swap_red_blue_in_place(bytes);
    }
    RenderImage::new(frames)
}
